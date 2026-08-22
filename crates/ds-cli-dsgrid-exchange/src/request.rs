//! Building one `ConversionRequest`, once.
//!
//! `plan` and `convert` take the same inputs and must build the same request
//! from them, because the whole point of planning first is that the plan you
//! read is the plan that runs. If the two commands assembled their requests
//! separately, a flag handled slightly differently in one of them would make
//! the plan a lie — and it would be a quiet lie, since both calls would still
//! succeed. So the request is built here and both commands call it.
//!
//! Every value this module accepts is a closed set, declared in `ARGS` and
//! enforced by the contract parser before a handler ever runs. A conversion
//! target is not a free-text field: an unrecognized one has to fail at the
//! door with the accepted list, not deep inside the engine.

use ds_cli_contract::Inputs;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Refusal};
use ds_grid_exchange::conversion::{
    BatchMode, ConversionRequest, ExpectedWgs84Location, PlsContainer, PlsVersionIntent, SourceSet,
    TargetFormat, resolve_pls_project_selection,
};
use serde_json::json;

/// The accepted `--target` values, in the order help prints them. These are
/// the CLI's spellings, not the engine's variant names: `shp` and `gpkg` are
/// what an operator types, and mapping them here is what keeps the engine's
/// Rust identifiers out of an interface people have to memorize.
pub const TARGETS: &[&str] = &[
    "dsgrid",
    "pls-folder",
    "pls-bak",
    "geojson",
    "kmz",
    "shp",
    "gpkg",
    "csv",
    "tsv",
    "xlsx",
];

pub const MODES: &[&str] = &["separate", "compose", "combine"];
pub const CONTAINERS: &[&str] = &["folder", "bak"];

/// The inputs `plan` and `convert` share. `convert` splices `--out` and
/// `--yes` onto this; nothing else differs between them, which is the
/// property that makes "plan it, then run exactly that" true.
pub const SHARED_ARGS: &[Arg] = &[
    crate::sources::SOURCE_ARG,
    Arg::value("target", "<format>", "The format to produce.")
        .required()
        .choices(TARGETS),
    Arg::value(
        "mode",
        "<mode>",
        "Treat sources separately, compose into one .dsgrid, or combine natively.",
    )
    .default("separate")
    .choices(MODES),
    Arg::value(
        "container",
        "<kind>",
        "For a PLS-CADD target: a workspace folder, or a .bak backup.",
    )
    .default("folder")
    .choices(CONTAINERS),
    Arg::value(
        "select-project",
        "<don-leaf>",
        "Pick one PLS project by its .don leaf when the sources hold several.",
    ),
    Arg::value(
        "crs",
        "<code>",
        "Declare the source CRS when the sources do not carry one.",
    ),
    Arg::switch("swap-xy", "Treat source coordinates as (y, x)."),
    Arg::value(
        "expect-lon",
        "<degrees>",
        "Assert where the result should land. Requires --expect-lat and --expect-radius-km.",
    ),
    Arg::value("expect-lat", "<degrees>", "See --expect-lon."),
    Arg::value(
        "expect-radius-km",
        "<km>",
        "How far from --expect-lon/--expect-lat is still acceptable.",
    ),
];

/// The refusals shared by request building. Spliced into both commands after
/// the source refusals, so the two help screens enumerate the same failures
/// in the same order.
pub const REQUEST_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "incomplete_expected_location",
        when: "only some of --expect-lon, --expect-lat and --expect-radius-km were given",
        remedy: "pass all three, or none",
    },
    Refusal {
        code: "invalid_expected_location",
        when: "an --expect-* value is not a number",
        remedy: "pass decimal degrees for lon/lat and kilometres for the radius",
    },
    Refusal {
        code: "project_not_resolvable",
        when: "--select-project names a .don leaf the sources do not hold, or hold ambiguously",
        remedy: "run `ds dsgrid-exchange inspect` to list what the sources actually contain",
    },
];

/// The version intent every native output is authored at.
///
/// There is deliberately no `--version` flag. `ds-grid` accepts one and then
/// rejects every value except 16.81, which makes it a flag whose only job is
/// to produce an error — the caller learns the constraint by tripping over
/// it. Stating the constraint in help instead costs one line and no failed
/// invocations. If this build ever authors a second version, the flag arrives
/// with a real closed set behind it.
const VERSION_INTENT: PlsVersionIntent = PlsVersionIntent::ConvertTo16_81;

/// The guard behind every closed-set flag in this module.
///
/// One code rather than three, because all three describe the same defect: a
/// value the contract parser accepted from a `choices` list that this build
/// has no engine value for. A caller cannot produce it — the choice list makes
/// it unreachable — which is why it is registered as internal-only in
/// `refusal_coverage.rs` rather than documented in three REFUSALS sections a
/// caller would never match on.
fn unmapped(flag: &str, given: &str, accepted: &[&str]) -> Failure {
    Failure::internal(
        "unmapped_choice",
        format!("--{flag} `{given}` passed validation but has no mapping in this build"),
    )
    .remedy(format!(
        "report this; accepted values are: {}",
        accepted.join(", ")
    ))
}

fn parse_target(value: &str) -> Result<TargetFormat, Failure> {
    Ok(match value {
        "dsgrid" => TargetFormat::Dsgrid,
        "pls-folder" => TargetFormat::PlsWorkspaceFolder,
        "pls-bak" => TargetFormat::PlsBackup,
        "geojson" => TargetFormat::GeoJson,
        "kmz" => TargetFormat::Kmz,
        "shp" => TargetFormat::ShapefileZip,
        "gpkg" => TargetFormat::GeoPackage,
        "csv" => TargetFormat::Csv,
        "tsv" => TargetFormat::Tsv,
        "xlsx" => TargetFormat::Xlsx,
        // Unreachable: the contract parser enforces `choices` before dispatch,
        // so only a value from TARGETS arrives here. Kept as a typed failure
        // rather than a panic so that adding a target to TARGETS without a
        // mapping here degrades to a clean refusal instead of a crash.
        other => return Err(unmapped("target", other, TARGETS)),
    })
}

fn parse_mode(value: Option<&str>) -> Result<BatchMode, Failure> {
    Ok(match value {
        None | Some("separate") => BatchMode::Separate,
        Some("compose") => BatchMode::ComposeDsGrid,
        Some("combine") => BatchMode::CombinePlsNative,
        other => return Err(unmapped("mode", other.unwrap_or_default(), MODES)),
    })
}

fn parse_container(value: Option<&str>) -> Result<PlsContainer, Failure> {
    Ok(match value {
        None | Some("folder") => PlsContainer::Folder,
        Some("bak") => PlsContainer::Backup,
        other => return Err(unmapped("container", other.unwrap_or_default(), CONTAINERS)),
    })
}

/// The three `--expect-*` flags are all-or-nothing on purpose. Two of the
/// three is not a weaker assertion — it is an assertion the engine cannot
/// evaluate, and accepting it would silently skip the check the caller
/// believed they had asked for.
fn parse_expected_location(inputs: &Inputs) -> Result<Option<ExpectedWgs84Location>, Failure> {
    let longitude = inputs.value("expect-lon");
    let latitude = inputs.value("expect-lat");
    let radius = inputs.value("expect-radius-km");

    match (longitude, latitude, radius) {
        (None, None, None) => Ok(None),
        (Some(longitude), Some(latitude), Some(radius)) => {
            let longitude_deg = number(longitude, "expect-lon")?;
            let latitude_deg = number(latitude, "expect-lat")?;
            let radius_km = number(radius, "expect-radius-km")?;
            Ok(Some(ExpectedWgs84Location {
                longitude_deg,
                latitude_deg,
                max_distance_m: radius_km * 1000.0,
            }))
        }
        _ => Err(Failure::invalid(
            "incomplete_expected_location",
            "--expect-lon, --expect-lat and --expect-radius-km must be given together",
        )
        .remedy("pass all three, or none")
        .detail(json!({
            "expect_lon": longitude.is_some(),
            "expect_lat": latitude.is_some(),
            "expect_radius_km": radius.is_some(),
        }))),
    }
}

fn number(raw: &str, flag: &str) -> Result<f64, Failure> {
    raw.parse::<f64>().map_err(|_| {
        Failure::invalid(
            "invalid_expected_location",
            format!("--{flag} `{raw}` is not a number"),
        )
        .remedy("pass decimal degrees for lon/lat and kilometres for the radius")
    })
}

/// Assemble the request. `sources` is passed in already loaded because
/// resolving `--select-project` needs to read the member list.
pub fn build(inputs: &Inputs, sources: SourceSet) -> Result<ConversionRequest, Failure> {
    let target = parse_target(inputs.require("target")?)?;
    let batch_mode = parse_mode(inputs.value("mode"))?;
    let pls_container = parse_container(inputs.value("container"))?;
    let expected_location = parse_expected_location(inputs)?;

    let pls_project = match inputs.value("select-project") {
        Some(leaf) => Some(
            resolve_pls_project_selection(&sources, leaf).map_err(|error| {
                Failure::invalid(
                    "project_not_resolvable",
                    format!("--select-project `{leaf}` did not resolve"),
                )
                .remedy("run `ds dsgrid-exchange inspect` to list what the sources hold")
                .next("ds dsgrid-exchange inspect --source <path>")
                .detail(json!({ "detail": error.to_string() }))
            })?,
        ),
        None => None,
    };

    Ok(ConversionRequest {
        sources,
        batch_mode,
        target,
        pls_version_intent: VERSION_INTENT,
        pls_container,
        // Explicit native combine ordering is not exposed in contract 1. The
        // engine accepts it; `ds` does not yet name it, and the reference doc
        // records that as a known gap rather than leaving a caller to guess
        // that the ordering is arbitrary. Default ordering is source order.
        combine: None,
        declared_crs: inputs.value("crs").map(str::to_string),
        expected_location,
        swap_xy: inputs.switch("swap-xy"),
        selection: Vec::new(),
        pls_project,
    })
}
