//! Exact Rwanda boundary reads and native hierarchy attachment.

use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_cli_desktop::ops::{
    AMBIGUOUS, BridgeOp, DESCRIPTOR_ARG, NOT_PAIRED, PAIRING_REJECTED, REFUSED, SIGNED_OUT,
    UNREACHABLE, UNREADABLE as DESKTOP_UNREADABLE, UNSUPPORTED as DESKTOP_UNSUPPORTED,
    classify_signed_out, invoke, paired, paired_availability,
};
use serde_json::{Map, Value, json};

pub const OPERATION: BridgeOp = BridgeOp {
    operation: "data.admin_bounds.attach",
    arguments: &["source", "out", "longitude_column", "latitude_column"],
};

pub const LIST_OPERATION: BridgeOp = BridgeOp {
    operation: "data.admin_bounds.list",
    arguments: &["country", "level", "parent_code"],
};

pub const READ_OPERATION: BridgeOp = BridgeOp {
    operation: "data.admin_bounds.read",
    arguments: &["country", "code", "to_map"],
};

const COUNTRY_ARG: Arg = Arg {
    name: "country",
    kind: ArgKind::Value,
    value: "<country>",
    required: false,
    default: Some("rwanda"),
    choices: &["rwanda"],
    summary: "Declared national boundary authority. Rwanda is the only installed authority.",
};
const LEVEL_ARG: Arg = Arg {
    name: "level",
    kind: ArgKind::Value,
    value: "<level>",
    required: true,
    default: None,
    choices: &["province", "district", "sector", "cell", "village"],
    summary: "Exact hierarchy level to list.",
};
const PARENT_CODE_ARG: Arg = Arg::value(
    "parent-code",
    "<code>",
    "Exact immediate parent code. Required below province.",
);
const CODE_ARG: Arg = Arg {
    name: "code",
    kind: ArgKind::Value,
    value: "<code>",
    required: true,
    default: None,
    choices: &[],
    summary: "Exact 1, 2, 4, 6, or 8 digit Rwanda administrative code.",
};
const TO_MAP_ARG: Arg = Arg::switch(
    "to-map",
    "Materialize the exact geometry through Desktop's normal local-layer and Style Center path.",
);

const INVALID_ADMIN_SCOPE: Refusal = Refusal {
    code: "invalid_admin_scope",
    when: "the country, level, code, or immediate parent relationship is not exact",
    remedy: "use country rwanda and the declared 1/2/4/6/8-digit hierarchy",
};
const ADMIN_AUTHORITY_UNAVAILABLE: Refusal = Refusal {
    code: "admin_authority_unavailable",
    when: "the authenticated administrative-boundary authority cannot answer the exact read",
    remedy: "restore the service connection and retry unchanged; do not approximate the geometry",
};
const ADMIN_AUTHORITY_UNREADABLE: Refusal = Refusal {
    code: "admin_authority_unreadable",
    when: "the authority returns malformed rows, mismatched identity, or non-polygon geometry",
    remedy: "retry once, then update DS GridDesign if the exact authority response remains unreadable",
};
const AUTH_CONTEXT_MISMATCH: Refusal = Refusal {
    code: "auth_context_mismatch",
    when: "the paired account, lane, audience, or project changes during the call",
    remedy: "confirm the paired identity context and retry the unchanged request",
};

const OUT_ARG: Arg = Arg {
    name: "out",
    kind: ArgKind::Value,
    value: "<path>",
    required: true,
    default: None,
    choices: &[],
    summary: "New same-format output path. Existing files are never overwritten.",
};
const SOURCE_ARG: Arg = Arg {
    name: "source",
    kind: ArgKind::Value,
    value: "<path>",
    required: true,
    default: None,
    choices: &[],
    summary: "Local CSV, TSV, GeoJSON, or JSON point file carrying elevation data.",
};
const LONGITUDE_ARG: Arg = Arg::value(
    "longitude-column",
    "<name>",
    "Longitude column for CSV/TSV input. Omit for GeoJSON geometry.",
);
const LATITUDE_ARG: Arg = Arg::value(
    "latitude-column",
    "<name>",
    "Latitude column for CSV/TSV input. Omit for GeoJSON geometry.",
);
const ADMIN_UNSUPPORTED: Refusal = Refusal {
    code: "source_unsupported",
    when: "the source is not CSV, TSV, or GeoJSON, or table coordinate columns were omitted",
    remedy: "use GeoJSON geometry, or pass both coordinate columns for CSV/TSV",
};
const NO_OVERWRITE: Refusal = Refusal {
    code: "output_refused",
    when: "the output already exists, is the source, or cannot be created safely",
    remedy: "choose a new same-format --out path; this command never overwrites",
};

pub static COMMAND: Command = Command {
    id: "data.admin-bounds.attach",
    path: &["data", "admin-bounds", "attach"],
    contract: 1,
    summary: "Attach Rwanda province-to-village fields to local elevation points.",
    purpose: "Uses the active project's digest-pinned Rwanda boundary resource and the bundled native reporter to write a new CSV, TSV, or GeoJSON file. The source geometry and elevation fields are unchanged; existing operator-supplied admin values win. This needs the paired desktop for its governed installed resource, but it does not need the map to be open.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        SOURCE_ARG,
        OUT_ARG,
        LONGITUDE_ARG,
        LATITUDE_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "The project, output path, matched/outside counts, output digest, reference digest, and attached columns.",
    examples: &[
        Example {
            command: "ds data admin-bounds attach --source ./elevation.csv --out ./elevation-admin.csv --longitude-column longitude --latitude-column latitude",
            note: "Streams table rows locally; no map is opened.",
            runnable: false,
        },
        Example {
            command: "ds data admin-bounds attach --source ./elevation.geojson --out ./elevation-admin.geojson",
            note: "GeoJSON reads coordinates from feature geometry.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::UNREADABLE,
        ADMIN_UNSUPPORTED,
        NO_OVERWRITE,
        NOT_PAIRED,
        AMBIGUOUS,
        UNREACHABLE,
        PAIRING_REJECTED,
        REFUSED,
        SIGNED_OUT,
        DESKTOP_UNREADABLE,
        DESKTOP_UNSUPPORTED,
    ],
    reference: Some("docs/reference/data.md"),
    availability: paired_availability,
};

pub static LIST_COMMAND: Command = Command {
    id: "data.admin-bounds.list",
    path: &["data", "admin-bounds", "list"],
    contract: 1,
    summary: "List exact Rwanda administrative units under one immediate parent.",
    purpose: "Reads the same authenticated national hierarchy used by Desktop Search place. Province is the root; every lower level requires its exact immediate parent code, keeping the result bounded and preventing a guessed hierarchy. This is country-scoped reference data, not project data, and no map needs to be open.",
    chapter: Chapter::Data,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[COUNTRY_ARG, LEVEL_ARG, PARENT_CODE_ARG, DESCRIPTOR_ARG],
    output: "Country/reference scope, the Desktop's active project as separate context, level, parent code, exact count, and bounded code/name rows. Geometry is not included.",
    examples: &[
        Example {
            command: "ds data admin-bounds list --country rwanda --level province --output json",
            note: "Lists the hierarchy root; no map or project is required.",
            runnable: false,
        },
        Example {
            command: "ds data admin-bounds list --country rwanda --level village --parent-code 110205 --output json",
            note: "Lists only the exact villages of one cell.",
            runnable: false,
        },
    ],
    refusals: &[
        INVALID_ADMIN_SCOPE,
        ADMIN_AUTHORITY_UNAVAILABLE,
        ADMIN_AUTHORITY_UNREADABLE,
        AUTH_CONTEXT_MISMATCH,
        NOT_PAIRED,
        AMBIGUOUS,
        UNREACHABLE,
        PAIRING_REJECTED,
        REFUSED,
        SIGNED_OUT,
        DESKTOP_UNREADABLE,
        DESKTOP_UNSUPPORTED,
    ],
    reference: Some("docs/reference/data.md"),
    availability: paired_availability,
};

pub static READ_COMMAND: Command = Command {
    id: "data.admin-bounds.read",
    path: &["data", "admin-bounds", "read"],
    contract: 1,
    summary: "Read one exact Rwanda boundary and bounded geometry evidence.",
    purpose: "Reads one code from the same authenticated geometry authority used by Desktop Search place. The polygon never travels through the CLI: the receipt reports identity, type, bounds, coordinate count and digest. With --to-map, Desktop materializes those exact bytes through its ordinary derived local-layer and hierarchy Style Center path; the local layer is not project data and works whether or not the map is already open.",
    chapter: Chapter::Data,
    effect: Effect::LocalUi,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[COUNTRY_ARG, CODE_ARG, TO_MAP_ARG, DESCRIPTOR_ARG],
    output: "Explicit national/project scope, exact boundary identity, geometry type/bounds/coordinate count/SHA-256, and an optional Desktop-local layer receipt. Full coordinates are never returned.",
    examples: &[
        Example {
            command: "ds data admin-bounds read --country rwanda --code 11020503 --output json",
            note: "Reads bounded evidence for one exact village without opening the map.",
            runnable: false,
        },
        Example {
            command: "ds data admin-bounds read --country rwanda --code 11020503 --to-map --output json",
            note: "Creates one normal Desktop-local styled boundary layer from the same exact geometry.",
            runnable: false,
        },
    ],
    refusals: &[
        INVALID_ADMIN_SCOPE,
        ADMIN_AUTHORITY_UNAVAILABLE,
        ADMIN_AUTHORITY_UNREADABLE,
        AUTH_CONTEXT_MISMATCH,
        NOT_PAIRED,
        AMBIGUOUS,
        UNREACHABLE,
        PAIRING_REJECTED,
        REFUSED,
        SIGNED_OUT,
        DESKTOP_UNREADABLE,
        DESKTOP_UNSUPPORTED,
    ],
    reference: Some("docs/reference/data.md"),
    availability: paired_availability,
};

fn invoke_read(
    operation: &BridgeOp,
    arguments: Map<String, Value>,
    inputs: &Inputs,
) -> Result<Value, Failure> {
    let descriptor = paired(inputs.value("desktop-descriptor"))?;
    invoke(
        &descriptor,
        operation,
        Value::Object(arguments),
        Duration::from_secs(2 * 60),
    )
    .map_err(classify_signed_out)
}

fn invalid_admin_scope(message: impl Into<String>) -> Failure {
    Failure::invalid("invalid_admin_scope", message).remedy(INVALID_ADMIN_SCOPE.remedy)
}

fn admin_country(raw: &str) -> Result<&'static str, Failure> {
    if raw.trim().eq_ignore_ascii_case("rwanda") {
        Ok("rwanda")
    } else {
        Err(invalid_admin_scope(
            "`--country` must be rwanda; no other boundary authority is declared",
        ))
    }
}

fn admin_level(raw: &str) -> Result<&'static str, Failure> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "province" => Ok("province"),
        "district" => Ok("district"),
        "sector" => Ok("sector"),
        "cell" => Ok("cell"),
        "village" => Ok("village"),
        _ => Err(invalid_admin_scope(
            "`--level` must be province, district, sector, cell, or village",
        )),
    }
}

fn exact_admin_code<'a>(raw: &'a str, flag: &str) -> Result<&'a str, Failure> {
    if matches!(raw.len(), 1 | 2 | 4 | 6 | 8) && raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(raw);
    }
    Err(invalid_admin_scope(format!(
        "`--{flag}` must be an exact 1, 2, 4, 6, or 8 digit Rwanda administrative code"
    )))
}

fn expected_parent_length(level: &str) -> Option<usize> {
    match level {
        "province" => None,
        "district" => Some(1),
        "sector" => Some(2),
        "cell" => Some(4),
        "village" => Some(6),
        _ => unreachable!("admin_level returns only declared levels"),
    }
}

pub fn run_list(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let country = admin_country(inputs.require("country")?)?;
    let level = admin_level(inputs.require("level")?)?;
    let parent = inputs
        .value("parent-code")
        .map(|raw| exact_admin_code(raw, "parent-code"))
        .transpose()?;
    match (expected_parent_length(level), parent) {
        (None, Some(_)) => {
            return Err(invalid_admin_scope(
                "province is the hierarchy root and does not accept `--parent-code`",
            ));
        }
        (Some(expected), None) => {
            return Err(invalid_admin_scope(format!(
                "{level} requires its exact immediate `--parent-code` ({expected} digits)"
            )));
        }
        (Some(expected), Some(code)) if code.len() != expected => {
            return Err(invalid_admin_scope(format!(
                "{level} `--parent-code` must be exactly {expected} digits"
            )));
        }
        _ => {}
    }
    let mut arguments = Map::new();
    arguments.insert("country".into(), json!(country));
    arguments.insert("level".into(), json!(level));
    if let Some(value) = parent {
        arguments.insert("parent_code".into(), json!(value));
    }
    invoke_read(&LIST_OPERATION, arguments, inputs)
}

pub fn run_read(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let country = admin_country(inputs.require("country")?)?;
    let code = exact_admin_code(inputs.require("code")?, "code")?;
    let mut arguments = Map::new();
    arguments.insert("country".into(), json!(country));
    arguments.insert("code".into(), json!(code));
    arguments.insert("to_map".into(), json!(inputs.switch("to-map")));
    invoke_read(&READ_OPERATION, arguments, inputs)
}

pub fn render_list(data: &Value) -> String {
    let mut out = format!(
        "{} {}{}\n",
        data["country"].as_str().unwrap_or("Rwanda"),
        data["level"].as_str().unwrap_or("boundaries"),
        data["parent_code"]
            .as_str()
            .map(|code| format!(" under {code}"))
            .unwrap_or_default(),
    );
    for row in data["boundaries"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<10} {}\n",
            row["code"].as_str().unwrap_or("?"),
            row["name"].as_str().unwrap_or("?"),
        ));
    }
    out
}

pub fn render_read(data: &Value) -> String {
    let boundary = &data["boundary"];
    let geometry = &data["geometry"];
    let mut out = format!(
        "{} {} {}\n  {} · {} coordinate positions · sha256 {}\n",
        boundary["level"].as_str().unwrap_or("boundary"),
        boundary["code"].as_str().unwrap_or("?"),
        boundary["name"].as_str().unwrap_or("?"),
        geometry["type"].as_str().unwrap_or("geometry"),
        geometry["coordinate_positions"].as_u64().unwrap_or(0),
        geometry["sha256"].as_str().unwrap_or("?"),
    );
    if data["materialized"].as_bool().unwrap_or(false) {
        out.push_str(&format!(
            "  local layer {}\n",
            data["local_layer"]["id"].as_str().unwrap_or("?")
        ));
    }
    out
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let source = std::fs::canonicalize(inputs.require("source")?).map_err(|error| {
        Failure::invalid(
            "source_unreadable",
            format!("Could not resolve the source: {error}"),
        )
        .remedy("Check the path exists and is readable.")
    })?;
    let raw_out = std::path::PathBuf::from(inputs.require("out")?);
    let out = if raw_out.is_absolute() {
        raw_out
    } else {
        std::env::current_dir()
            .map_err(|error| Failure::internal("output_refused", error.to_string()))?
            .join(raw_out)
    };
    if out.exists() {
        return Err(
            Failure::conflict("output_refused", "The output file already exists.")
                .remedy("Choose another --out path; admin attachment never overwrites."),
        );
    }
    let longitude = inputs.value("longitude-column");
    let latitude = inputs.value("latitude-column");
    if longitude.is_some() != latitude.is_some() {
        return Err(Failure::invalid(
            "source_unsupported",
            "--longitude-column and --latitude-column must be supplied together.",
        ));
    }
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "csv" | "tsv") && longitude.is_none() {
        return Err(Failure::invalid(
            "source_unsupported",
            "CSV/TSV admin attachment requires explicit coordinate columns.",
        )
        .remedy("Pass --longitude-column and --latitude-column from `ds data inspect`."));
    }
    if !matches!(extension.as_str(), "csv" | "tsv" | "geojson" | "json") {
        return Err(Failure::invalid(
            "source_unsupported",
            "Admin attachment supports CSV, TSV, and GeoJSON point files.",
        ));
    }
    let mut arguments = Map::new();
    arguments.insert("source".into(), json!(source.to_string_lossy()));
    arguments.insert("out".into(), json!(out.to_string_lossy()));
    if let Some(value) = longitude {
        arguments.insert("longitude_column".into(), json!(value));
    }
    if let Some(value) = latitude {
        arguments.insert("latitude_column".into(), json!(value));
    }
    let descriptor = paired(inputs.value("desktop-descriptor"))?;
    invoke(
        &descriptor,
        &OPERATION,
        Value::Object(arguments),
        Duration::from_secs(20 * 60),
    )
    .map_err(classify_signed_out)
}

pub fn render(data: &Value) -> String {
    format!(
        "attached Rwanda admin bounds to {}\n  {} of {} points matched · sha256 {}\n",
        data["out"].as_str().unwrap_or("?"),
        data["features_matched"].as_u64().unwrap_or(0),
        data["features_read"].as_u64().unwrap_or(0),
        data["output_sha256"].as_str().unwrap_or("?"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_scope_validation_has_one_stable_code() {
        for failure in [
            admin_country("kenya").unwrap_err(),
            admin_level("county").unwrap_err(),
            exact_admin_code("110", "code").unwrap_err(),
            exact_admin_code("11x2", "parent-code").unwrap_err(),
        ] {
            assert_eq!(failure.code(), "invalid_admin_scope");
            assert_eq!(failure.class().token(), "invalid_input");
        }
    }

    #[test]
    fn immediate_parent_lengths_match_the_prefix_hierarchy() {
        assert_eq!(expected_parent_length("province"), None);
        assert_eq!(expected_parent_length("district"), Some(1));
        assert_eq!(expected_parent_length("sector"), Some(2));
        assert_eq!(expected_parent_length("cell"), Some(4));
        assert_eq!(expected_parent_length("village"), Some(6));
    }
}
