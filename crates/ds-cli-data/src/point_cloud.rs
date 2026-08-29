//! `ds data elevation plan` / `ds data elevation extract` — Desktop elevation
//! point-cloud extraction from an area.
//!
//! A DIFFERENT operation from `ds data elevation attach`, which enriches a
//! point file the caller already has. These two take an AREA and produce the
//! points. Someone preparing a grading exercise needs the second and cannot get
//! there with the first, which is why they are separate commands rather than a
//! flag.
//!
//! Both are map-independent by construction: an area is a local file or four
//! numbers, never a selection on a screen. That is what makes them usable from
//! a script, from CI, and through MCP.
//!
//! `plan` reads no DEM byte and writes nothing — it answers exactly how many
//! points the request produces and where a job of that size runs. `extract`
//! generates, samples locally, and writes GeoJSON plus CSV. Splitting them is
//! the whole point: a caller learns the size before paying for it.

use std::path::Path;
use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_cli_desktop::ops::{
    AMBIGUOUS, BridgeOp, DESCRIPTOR_ARG, NOT_PAIRED, PAIRING_REJECTED, REFUSED, UNREACHABLE,
    UNREADABLE as DESKTOP_UNREADABLE, UNSUPPORTED as DESKTOP_UNSUPPORTED, invoke, paired,
    paired_availability,
};
use serde_json::{Map, Value, json};

pub const PLAN_OPERATION: BridgeOp = BridgeOp {
    operation: "data.elevation.plan",
    arguments: &[
        "area",
        "bbox",
        "mode",
        "spacing_m",
        "seed",
        "count",
        "density_per_km2",
    ],
};

pub const EXTRACT_OPERATION: BridgeOp = BridgeOp {
    operation: "data.elevation.extract",
    arguments: &[
        "area",
        "bbox",
        "out",
        "mode",
        "spacing_m",
        "seed",
        "count",
        "density_per_km2",
        "fallback",
    ],
};

const AREA_ARG: Arg = Arg::value(
    "area",
    "<absolute-path>",
    "Absolute local area file: GeoJSON, KML/KMZ, or a zipped Shapefile. Every polygon in it is one area.",
);
const BBOX_ARG: Arg = Arg::value(
    "bbox",
    "<w,s,e,n>",
    "Area as four numbers: west,south,east,north in WGS84 degrees. Needs no file and no map.",
);
const MODE_ARG: Arg = Arg {
    name: "mode",
    kind: ArgKind::Value,
    value: "<mode>",
    required: true,
    default: None,
    choices: &["grid", "seeded_random"],
    summary: "Regular lattice at an explicit spacing, or reproducible pseudo-random placement.",
};
const SPACING_ARG: Arg = Arg::value(
    "spacing-m",
    "<metres>",
    "Grid mode only: lattice spacing in metres, at least 0.5.",
);
const SEED_ARG: Arg = Arg::value(
    "seed",
    "<integer>",
    "Seeded-random only: the same seed over the same area returns the same points.",
);
const COUNT_ARG: Arg = Arg::value(
    "count",
    "<integer>",
    "Seeded-random only: exact points per area feature. Use this or --density-per-km2, not both.",
);
const DENSITY_ARG: Arg = Arg::value(
    "density-per-km2",
    "<number>",
    "Seeded-random only: points per km² of each area's true, hole-aware surface.",
);
const OUT_ARG: Arg = Arg {
    name: "out",
    kind: ArgKind::Value,
    value: "<absolute-path.geojson>",
    required: true,
    default: None,
    choices: &[],
    summary: "Absolute path for a new GeoJSON result; the CSV is written beside it. Existing files are never overwritten.",
};
const FALLBACK_ARG: Arg = Arg {
    name: "fallback",
    kind: ArgKind::Value,
    value: "<source>",
    required: false,
    default: Some("terrarium"),
    choices: &["terrarium", "none"],
    summary: "Use explicit AWS Terrarium fallback outside Rwanda DEM coverage, or none.",
};

const AREA_REQUIRED: Refusal = Refusal {
    code: "area_required",
    when: "neither --area nor --bbox was given, or --bbox is not four ordered finite numbers",
    remedy: "pass --area <absolute-path> or --bbox \"west,south,east,north\"",
};
const SAMPLING_INCOMPLETE: Refusal = Refusal {
    code: "sampling_incomplete",
    when: "the sampling choice is under-specified: grid without --spacing-m, seeded_random without --seed, or neither/both of --count and --density-per-km2",
    remedy: "state the sampling choice in full; it is never inferred, because a guessed density is a result nobody asked for",
};
const ABSOLUTE_PATH_REQUIRED: Refusal = Refusal {
    code: "absolute_path_required",
    when: "--area or --out is not an absolute local path",
    remedy: "resolve the paths on the Desktop machine before invoking the command",
};
const FULL_LOCAL_DEM_REQUIRED: Refusal = Refusal {
    code: "full_local_dem_required",
    when: "more than 4,000 generated points are requested without the verified full Rwanda DEM component",
    remedy: "retry unchanged — the Desktop component manager installs and verifies the full local DEM once — or reduce the area",
};
const POINT_BUDGET_EXCEEDED: Refusal = Refusal {
    code: "point_budget_exceeded",
    when: "the area and sampling choice generate more points than one extraction materializes",
    remedy: "widen --spacing-m, lower --density-per-km2, or split the area into smaller extractions",
};
const AREA_TOO_LARGE: Refusal = Refusal {
    code: "area_too_large_to_plan",
    when: "the area and sampling choice are too large for the planner to walk at all",
    remedy: "widen --spacing-m or lower --density-per-km2; a larger machine does not help here",
};
const CANCELLED: Refusal = Refusal {
    code: "extraction_cancelled",
    when: "the operator cancelled the extraction in the application",
    remedy: "run it again when ready; nothing was written",
};

pub static PLAN_COMMAND: Command = Command {
    id: "data.elevation.plan",
    path: &["data", "elevation", "plan"],
    contract: 1,
    summary: "Count the elevation points an area and sampling choice would produce.",
    purpose: "Answers exactly how many points an extraction generates, per area and in total, and where a job of that size runs. Reads no elevation data and writes nothing, so a job's size is known before it is paid for. The area is a local file or a bounding box, so no map need be open. This is the preview half of `ds data elevation extract`.",
    chapter: Chapter::Data,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        AREA_ARG,
        BBOX_ARG,
        MODE_ARG,
        SPACING_ARG,
        SEED_ARG,
        COUNT_ARG,
        DENSITY_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "Exact total and per-area point counts, each area's surface in km², whether the count is admissible in the cloud lane, and whether the full local Rwanda DEM is required.",
    examples: &[
        Example {
            command: "ds data elevation plan --bbox \"30.05,-1.95,30.06,-1.94\" --mode grid --spacing-m 50",
            note: "Hypothetical Kigali block; needs no file and no map.",
            runnable: false,
        },
        Example {
            command: "ds data elevation plan --area /data/sector.geojson --mode seeded_random --seed 2026 --density-per-km2 250",
            note: "Hypothetical sector boundary; each area is billed for its true hole-aware surface.",
            runnable: false,
        },
    ],
    refusals: &[
        AREA_REQUIRED,
        SAMPLING_INCOMPLETE,
        ABSOLUTE_PATH_REQUIRED,
        POINT_BUDGET_EXCEEDED,
        AREA_TOO_LARGE,
        NOT_PAIRED,
        AMBIGUOUS,
        UNREACHABLE,
        PAIRING_REJECTED,
        REFUSED,
        DESKTOP_UNREADABLE,
        DESKTOP_UNSUPPORTED,
    ],
    reference: Some("docs/reference/data.md"),
    availability: paired_availability,
};

pub static EXTRACT_COMMAND: Command = Command {
    id: "data.elevation.extract",
    path: &["data", "elevation", "extract"],
    contract: 1,
    summary: "Extract an elevation point cloud from an area on Desktop.",
    purpose: "Generates points across an area, attaches Rwanda DEM elevation locally, and writes a new GeoJSON with a CSV beside it. Different from `ds data elevation attach`, which enriches points the caller already has: this one produces them. Generation is deterministic and every source area attribute is preserved. Jobs above 4,000 points install or verify the full local DEM once, then retry. Nothing reaches DS Cloud Run and no map need be open.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        AREA_ARG,
        BBOX_ARG,
        OUT_ARG,
        MODE_ARG,
        SPACING_ARG,
        SEED_ARG,
        COUNT_ARG,
        DENSITY_ARG,
        FALLBACK_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "A native receipt: both written paths and digests, point and coverage counts, DEM access mode, per-area statistics, and any displaced source attribute.",
    examples: &[
        Example {
            command: "ds data elevation extract --bbox \"30.05,-1.95,30.06,-1.94\" --out /data/kigali-elevation.geojson --mode grid --spacing-m 25",
            note: "Hypothetical 25 m grading grid; writes GeoJSON plus kigali-elevation.csv.",
            runnable: false,
        },
        Example {
            command: "ds data elevation extract --area /data/sector.geojson --out /data/sector-points.geojson --mode seeded_random --seed 2026 --count 2000 --fallback none",
            note: "Hypothetical reproducible sample; without a fallback, points outside Rwanda coverage stay explicit gaps.",
            runnable: false,
        },
    ],
    refusals: &[
        AREA_REQUIRED,
        SAMPLING_INCOMPLETE,
        ABSOLUTE_PATH_REQUIRED,
        FULL_LOCAL_DEM_REQUIRED,
        POINT_BUDGET_EXCEEDED,
        AREA_TOO_LARGE,
        CANCELLED,
        NOT_PAIRED,
        AMBIGUOUS,
        UNREACHABLE,
        PAIRING_REJECTED,
        REFUSED,
        DESKTOP_UNREADABLE,
        DESKTOP_UNSUPPORTED,
    ],
    reference: Some("docs/reference/data.md"),
    availability: paired_availability,
};

fn absolute(inputs: &Inputs, name: &str) -> Result<Option<String>, Failure> {
    let Some(raw) = inputs.value(name) else {
        return Ok(None);
    };
    if !Path::new(raw).is_absolute() {
        return Err(Failure::invalid(
            "absolute_path_required",
            format!("--{name} must be an absolute path on the paired Desktop machine."),
        )
        .remedy(ABSOLUTE_PATH_REQUIRED.remedy));
    }
    Ok(Some(raw.to_string()))
}

fn incomplete(message: impl Into<String>) -> Failure {
    Failure::invalid("sampling_incomplete", message).remedy(SAMPLING_INCOMPLETE.remedy)
}

/// Collect the arguments both operations share.
///
/// Nothing is defaulted here. A missing spacing or seed is a refusal, because
/// a sampling choice this command invented is a result the caller never asked
/// for and has no way to notice.
fn common_arguments(inputs: &Inputs) -> Result<Map<String, Value>, Failure> {
    let mut arguments = Map::new();
    let area = absolute(inputs, "area")?;
    let bbox = inputs.value("bbox").map(str::to_string);
    if area.is_none() && bbox.is_none() {
        return Err(Failure::invalid(
            "area_required",
            "An elevation point cloud needs an area: pass --area or --bbox.",
        )
        .remedy(AREA_REQUIRED.remedy));
    }
    if let Some(area) = area {
        arguments.insert("area".into(), json!(area));
    }
    if let Some(bbox) = bbox {
        arguments.insert("bbox".into(), json!(bbox));
    }

    let mode = inputs.require("mode")?;
    arguments.insert("mode".into(), json!(mode));
    match mode {
        "grid" => {
            let spacing = inputs.value("spacing-m").ok_or_else(|| {
                incomplete("Grid sampling requires --spacing-m; it is never inferred.")
            })?;
            arguments.insert("spacing_m".into(), json!(spacing));
        }
        "seeded_random" => {
            let seed = inputs.value("seed").ok_or_else(|| {
                incomplete(
                    "Seeded-random sampling requires --seed so the result can be reproduced.",
                )
            })?;
            arguments.insert("seed".into(), json!(seed));
            match (inputs.value("count"), inputs.value("density-per-km2")) {
                (Some(_), Some(_)) => {
                    return Err(incomplete(
                        "Give --count or --density-per-km2, not both; they mean different things.",
                    ));
                }
                (Some(count), None) => {
                    arguments.insert("count".into(), json!(count));
                }
                (None, Some(density)) => {
                    arguments.insert("density_per_km2".into(), json!(density));
                }
                (None, None) => {
                    return Err(incomplete(
                        "Seeded-random sampling requires --count or --density-per-km2.",
                    ));
                }
            }
        }
        other => {
            return Err(incomplete(format!(
                "--mode must be grid or seeded_random, got '{other}'."
            )));
        }
    }
    Ok(arguments)
}

/// Re-raise the application's refusal under its own code, so a script branches
/// on what actually happened rather than on a generic failure.
fn refusal(result: &Value) -> Result<Value, Failure> {
    let refusal = &result["refusal"];
    let code = refusal["code"].as_str().unwrap_or("desktop_refused");
    let message = refusal["message"]
        .as_str()
        .unwrap_or("Desktop refused the elevation extraction without a valid explanation.");
    let remedy = refusal["remedy"]
        .as_str()
        .unwrap_or("Update DS GridDesign and retry with the documented command contract.");
    let detail = json!({
        "point_count": refusal.get("point_count").cloned().unwrap_or(Value::Null),
        "point_limit": refusal.get("point_limit").cloned().unwrap_or(Value::Null),
    });
    let failure = match code {
        "full_local_dem_required" => Failure::unavailable("full_local_dem_required", message),
        "point_budget_exceeded" => Failure::invalid("point_budget_exceeded", message),
        "area_too_large_to_plan" => Failure::invalid("area_too_large_to_plan", message),
        "cancelled" => Failure::failed("extraction_cancelled", message),
        _ => Failure::failed("desktop_refused", message),
    };
    Err(failure.remedy(remedy).detail(detail))
}

pub fn run_plan(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let arguments = common_arguments(inputs)?;
    let descriptor = paired(inputs.value("desktop-descriptor"))?;
    let result = invoke(
        &descriptor,
        &PLAN_OPERATION,
        Value::Object(arguments),
        Duration::from_secs(10 * 60),
    )?;
    match result["status"].as_str() {
        Some("planned") if result["receipt"].is_object() => Ok(result["receipt"].clone()),
        Some("refused") => refusal(&result),
        _ => Err(Failure::failed(
            "desktop_refused",
            "Desktop returned an invalid elevation point-cloud plan outcome.",
        )
        .remedy("Update DS GridDesign and retry with the same explicit area and sampling choice.")),
    }
}

pub fn run_extract(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = common_arguments(inputs)?;
    let out = absolute(inputs, "out")?.ok_or_else(|| {
        Failure::invalid("absolute_path_required", "--out is required.")
            .remedy(ABSOLUTE_PATH_REQUIRED.remedy)
    })?;
    arguments.insert("out".into(), json!(out));
    arguments.insert("fallback".into(), json!(inputs.require("fallback")?));

    let descriptor = paired(inputs.value("desktop-descriptor"))?;
    let result = invoke(
        &descriptor,
        &EXTRACT_OPERATION,
        Value::Object(arguments),
        Duration::from_secs(10 * 60 * 60),
    )?;
    match result["status"].as_str() {
        Some("completed") if result["receipt"].is_object() => Ok(result["receipt"].clone()),
        Some("refused") => refusal(&result),
        _ => Err(Failure::failed(
            "desktop_refused",
            "Desktop returned an invalid elevation point-cloud outcome.",
        )
        .remedy("Update DS GridDesign and retry with the same explicit area and output.")),
    }
}

pub fn render_plan(data: &Value) -> String {
    let mut out = format!(
        "{} points across {} areas ({:.3} km²), sampled by {}\n",
        data["point_count"].as_u64().unwrap_or(0),
        data["area_count"].as_u64().unwrap_or(0),
        data["total_area_km2"].as_f64().unwrap_or(0.0),
        data["sampling"].as_str().unwrap_or("?"),
    );
    // Where it runs is the reason to plan at all, so it is never omitted.
    out.push_str(&format!(
        "  cloud lane: {} (limit {}) · full local DEM required: {}\n",
        if data["cloud_admissible"].as_bool().unwrap_or(false) {
            "admissible"
        } else {
            "too large"
        },
        data["max_cloud_points"].as_u64().unwrap_or(0),
        data["full_local_dem_required"].as_bool().unwrap_or(false),
    ));
    for area in data["areas"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {}: {} points · {:.3} km² · {} part(s)\n",
            area["area_id"].as_str().unwrap_or("?"),
            area["point_count"].as_u64().unwrap_or(0),
            area["area_km2"].as_f64().unwrap_or(0.0),
            area["part_count"].as_u64().unwrap_or(0),
        ));
    }
    out
}

pub fn render_extract(data: &Value) -> String {
    let mut out = format!(
        "extracted {} elevation points to {}\n  {} · Rwanda {} · fallback {} · sha256 {}\n  csv {} · sha256 {}\n",
        data["point_count"].as_u64().unwrap_or(0),
        data["output_path"].as_str().unwrap_or("?"),
        data["sampling"].as_str().unwrap_or("?"),
        data["rwanda_coverage"].as_str().unwrap_or("?"),
        data["fallback_count"].as_u64().unwrap_or(0),
        data["output_sha256"].as_str().unwrap_or("?"),
        data["csv_path"].as_str().unwrap_or("?"),
        data["csv_sha256"].as_str().unwrap_or("?"),
    );
    // A displaced source attribute is reported, never left for the caller to
    // discover by reading the file.
    if let Some(renamed) = data["renamed_attributes"].as_object() {
        for (from, to) in renamed {
            out.push_str(&format!(
                "  source attribute {from} preserved as {}\n",
                to.as_str().unwrap_or("?")
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_operations_are_closed_and_mapless() {
        assert_eq!(PLAN_OPERATION.operation, "data.elevation.plan");
        assert_eq!(EXTRACT_OPERATION.operation, "data.elevation.extract");
        // An area is a file or four numbers, never a map selection.
        for operation in [&PLAN_OPERATION, &EXTRACT_OPERATION] {
            assert!(operation.arguments.contains(&"area"));
            assert!(operation.arguments.contains(&"bbox"));
            assert!(!operation.arguments.contains(&"layer"));
            assert!(!operation.arguments.contains(&"selection"));
        }
    }

    #[test]
    fn planning_reads_and_extraction_writes() {
        assert_eq!(PLAN_COMMAND.effect, Effect::ReadOnly);
        assert_eq!(EXTRACT_COMMAND.effect, Effect::LocalFileWrite);
        assert_eq!(PLAN_COMMAND.authority, Authority::DesktopPairing);
        assert_eq!(EXTRACT_COMMAND.authority, Authority::DesktopPairing);
    }

    #[test]
    fn extraction_never_takes_a_point_source() {
        // The distinction from `attach` has to hold in the arguments, not only
        // in the prose: an input named --source would blur exactly the line
        // these commands exist to draw.
        for operation in [&PLAN_OPERATION, &EXTRACT_OPERATION] {
            assert!(!operation.arguments.contains(&"source"));
        }
        for command in [&PLAN_COMMAND, &EXTRACT_COMMAND] {
            assert!(command.args.iter().all(|arg| arg.name != "source"));
        }
    }

    #[test]
    fn every_native_refusal_is_declared_with_a_remedy() {
        for code in [
            "area_required",
            "sampling_incomplete",
            "point_budget_exceeded",
            "area_too_large_to_plan",
        ] {
            assert!(
                PLAN_COMMAND.refusals.iter().any(|r| r.code == code),
                "{code}"
            );
        }
        for code in [
            "full_local_dem_required",
            "extraction_cancelled",
            "point_budget_exceeded",
            "area_too_large_to_plan",
        ] {
            assert!(
                EXTRACT_COMMAND.refusals.iter().any(|r| r.code == code),
                "{code}"
            );
        }
        for command in [&PLAN_COMMAND, &EXTRACT_COMMAND] {
            for refusal in command.refusals {
                assert!(!refusal.remedy.is_empty(), "{} has no remedy", refusal.code);
            }
        }
    }

    #[test]
    fn the_bridge_argument_sets_match_what_the_commands_can_send() {
        // A key the adapter would reject is a runtime failure with a clean
        // compile, so the two lists are compared rather than trusted.
        let plan_flags = [
            "area",
            "bbox",
            "mode",
            "spacing_m",
            "seed",
            "count",
            "density_per_km2",
        ];
        assert_eq!(PLAN_OPERATION.arguments, &plan_flags);
        let extract_flags = [
            "area",
            "bbox",
            "out",
            "mode",
            "spacing_m",
            "seed",
            "count",
            "density_per_km2",
            "fallback",
        ];
        assert_eq!(EXTRACT_OPERATION.arguments, &extract_flags);
    }
}
