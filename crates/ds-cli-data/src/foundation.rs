//! Cloud-resident reference data, read where it lives.
//!
//! `rwanda_upi_parcels` (11.5 million cadastral polygons) and
//! `edcl_customers` (one million anonymized connections) are never bundled,
//! seeded or downloaded — BigQuery is their only holding
//! (`docs/contracts/foundation-datasets.md`). These two commands are the
//! bounded reads `ds` makes of them through ds-brain's data-distribution
//! surface: one UPI → one parcel polygon; the customers of one village, one
//! cell, one small rectangle, or one transformer's design area, capped at
//! 5,000 and receipted with the dataset, the query, the bound and the counts.
//!
//! Both run headlessly under the restored native user against the fenced
//! selected project; membership is ds-brain's decision. As with
//! `data admin-bounds read`, the terminal prints evidence rather than
//! coordinates; `--geometry-out` keeps the exact GeoJSON as a file that
//! `ds map local register` takes as it stands.

use std::io::Write;
use std::path::{Path, PathBuf};

use ds_cli_auth::DataDistributionRequest;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::project_dataset_cache::{self as policy, Scope, buffer_policy};
use serde_json::{Value, json};

const UPI_ARG: Arg = Arg {
    name: "upi",
    kind: ArgKind::Value,
    value: "<upi>",
    required: true,
    default: None,
    choices: &[],
    summary: "The parcel's UPI as printed on its title: 8 to 20 digits, slashes allowed (2/05/06/01/2183).",
};
const VILLAGE_ARG: Arg = Arg::value(
    "village",
    "<8-digit code>",
    "Exact village code from `ds data admin-bounds list --level village`.",
);
const CELL_ARG: Arg = Arg::value(
    "cell",
    "<6-digit code>",
    "Exact cell code from `ds data admin-bounds list --level cell`.",
);
const BBOX_ARG: Arg = Arg::value(
    "bbox",
    "<west,south,east,north>",
    "A WGS84 rectangle of at most 25 km².",
);
const TRANSFORMER_ARG: Arg = Arg::value(
    "transformer",
    "<name>",
    "One active transformer of the selected project: its design extent, buffered by the project's design buffer, is the rectangle.",
);
const BOUNDARY_ARG: Arg = Arg::value(
    "boundary",
    "<path.geojson>",
    "One WGS84 Polygon or MultiPolygon (bare, a Feature, or a one-feature FeatureCollection) whose envelope is at most 25 km²: a corridor from `ds data vector buffer`, an admin unit, a drawn extent.",
);
const LIMIT_ARG: Arg = Arg::value("limit", "<1-5000>", "Most rows to answer; 5000 is the cap.")
    .default("5000");
const GEOMETRY_OUT_ARG: Arg = Arg::value(
    "geometry-out",
    "<path.geojson>",
    "Also write the exact GeoJSON here (a FeatureCollection). Existing files are never overwritten.",
);
const LANE_ARG: Arg = Arg::value("lane", "<stable|canary>", "Native authentication lane.")
    .default("stable")
    .choices(&["stable", "canary"]);

macro_rules! refusal {
    ($name:ident, $code:literal, $when:literal, $remedy:literal) => {
        const $name: Refusal = Refusal {
            code: $code,
            when: $when,
            remedy: $remedy,
        };
    };
}
refusal!(
    UPI_INVALID,
    "upi_invalid",
    "the UPI is not 8 to 20 digits (slashes allowed)",
    "pass the parcel's UPI as printed on its title"
);
refusal!(
    UPI_NOT_FOUND,
    "upi_not_found",
    "the parcels authority holds no parcel with that UPI",
    "check the UPI; parcels are as published by the land-registry snapshot the catalogue names"
);
refusal!(
    AMBIGUOUS,
    "dataset_ambiguous",
    "more than one parcel carries the UPI",
    "report it as a data defect through ds feedback; nothing is chosen for you"
);
refusal!(
    BOUND_EXCEEDED,
    "bound_exceeded",
    "--limit is above 5000, or the rectangle is above 25 km²",
    "narrow the area, or name a village or cell"
);
refusal!(
    INVALID_SCOPE,
    "invalid_admin_scope",
    "not exactly one of --village, --cell, --bbox, --transformer was given, or a code is not 8/6 digits",
    "name one bound with the exact code from `ds data admin-bounds list`"
);
refusal!(
    GEOMETRY_OUT_REFUSED,
    "output_refused",
    "--geometry-out already exists, or its directory cannot be written",
    "choose a new --geometry-out path; a read never overwrites a file"
);
refusal!(
    NOT_FOUND,
    "transformer_not_found",
    "the named transformer is not an active transformer of the selected project",
    "pass one exact transformer name from `ds design status`"
);
refusal!(
    NO_DESIGN_EXTENT,
    "project_has_no_extent",
    "the named transformer has no saved design to derive a rectangle from",
    "save the transformer's design, or name a village, cell or rectangle"
);

/// The native-user refusals every headless read can return, composed after
/// this command's own (as `admin_bounds` composes them).
const AUTH_REFUSALS: usize = ds_cli_auth::PROJECT_LIST_COMMAND.refusals.len();
const fn with_native_refusals<const N: usize, const TOTAL: usize>(
    own: [Refusal; N],
) -> [Refusal; TOTAL] {
    let mut all = [UPI_INVALID; TOTAL];
    let mut index = 0;
    while index < N {
        all[index] = own[index];
        index += 1;
    }
    index = 0;
    while index < AUTH_REFUSALS {
        all[N + index] = ds_cli_auth::PROJECT_LIST_COMMAND.refusals[index];
        index += 1;
    }
    all
}
const LOOKUP_REFUSAL_SET: [Refusal; 5 + AUTH_REFUSALS] =
    with_native_refusals::<5, { 5 + AUTH_REFUSALS }>([
        UPI_INVALID,
        UPI_NOT_FOUND,
        AMBIGUOUS,
        GEOMETRY_OUT_REFUSED,
        ds_cli_auth::DATA_DISTRIBUTION_UNAVAILABLE_REFUSAL,
    ]);
const LOOKUP_REFUSALS: &[Refusal] = &LOOKUP_REFUSAL_SET;
const QUERY_REFUSAL_SET: [Refusal; 6 + AUTH_REFUSALS] =
    with_native_refusals::<6, { 6 + AUTH_REFUSALS }>([
        INVALID_SCOPE,
        BOUND_EXCEEDED,
        NOT_FOUND,
        NO_DESIGN_EXTENT,
        GEOMETRY_OUT_REFUSED,
        ds_cli_auth::DATA_DISTRIBUTION_UNAVAILABLE_REFUSAL,
    ]);
const QUERY_REFUSALS: &[Refusal] = &QUERY_REFUSAL_SET;

pub static LOOKUP_COMMAND: Command = Command {
    id: "data.upi.lookup",
    path: &["data", "upi", "lookup"],
    contract: 1,
    summary: "Find one land parcel by its UPI in the cloud parcels dataset.",
    purpose: "Reads exactly one cadastral parcel from the Rwanda parcels authority (11.5 million parcels in BigQuery, never installed nationally). The answer is a receipt naming the dataset, its source table and version, the bound (the UPI), and the one feature: its geometry plus province, district, sector, cell, village, parcel key, area and dates. The geometry is not printed; pass --geometry-out to keep it as a one-feature GeoJSON layer.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[UPI_ARG, GEOMETRY_OUT_ARG, LANE_ARG],
    output: "Dataset (id, layer, source, residency cloud, version), the query bound, rows_cap/rows_returned/rows_total/truncated, the parcel's properties, bounds and vertex count, and the written path when --geometry-out was given.",
    examples: &[
        Example {
            command: "ds data upi lookup --upi 20506012183 --output json",
            note: "One parcel, one feature, receipted; costs one bounded BigQuery read.",
            runnable: false,
        },
        Example {
            command: "ds data upi lookup --upi 2/05/06/01/2183 --geometry-out ./parcel-2183.geojson",
            note: "The printed UPI form is accepted; the exact geometry is kept as a layer file.",
            runnable: false,
        },
    ],
    refusals: LOOKUP_REFUSALS,
    reference: Some("docs/reference/data.md"),
    search: &["cadastral", "plot", "title", "foundation", "bigquery"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static QUERY_COMMAND: Command = Command {
    id: "data.customers.query",
    path: &["data", "customers", "query"],
    contract: 1,
    summary: "List existing customers in one village, cell, area or transformer.",
    purpose: "Reads the anonymized EDCL customers (one million points in BigQuery, never installed nationally) inside exactly one bound: a village or cell (within its authority boundary), a rectangle of at most 25 km², a boundary (a design buffer, an admin unit) or one transformer's buffered design extent. Capped at 5,000 rows; the receipt states rows_total and whether the answer was truncated. Customers carry segmentation, meter type and category, payment method, connection year and administrative names. To keep them with the project like building footprints: `ds data project-cache seed --dataset edcl_customers`.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        VILLAGE_ARG,
        CELL_ARG,
        BBOX_ARG,
        BOUNDARY_ARG,
        TRANSFORMER_ARG,
        LIMIT_ARG,
        GEOMETRY_OUT_ARG,
        LANE_ARG,
    ],
    output: "Dataset (id, layer, source, residency cloud, version), the query bound, rows_cap/rows_returned/rows_total/truncated, counts per village and per segmentation, and the written path when --geometry-out was given.",
    examples: &[
        Example {
            command: "ds data customers query --village 25060102 --output json",
            note: "Every customer inside one village boundary, up to the cap.",
            runnable: false,
        },
        Example {
            command: "ds data customers query --transformer fill_in_kabuhoro --limit 500 --geometry-out ./customers.geojson",
            note: "Customers in the transformer's design area, kept as a point layer.",
            runnable: false,
        },
        Example {
            command: "ds data customers query --boundary ./corridor.geojson --output json",
            note: "Customers inside a buffered line corridor written by `ds data vector buffer`.",
            runnable: false,
        },
    ],
    refusals: QUERY_REFUSALS,
    reference: Some("docs/reference/data.md"),
    search: &["meter", "edcl", "household", "served", "foundation", "cloud", "bigquery"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static PARCELS_COMMAND: Command = Command {
    id: "data.parcels.query",
    path: &["data", "parcels", "query"],
    contract: 1,
    summary: "List UPI parcels in one village, cell, area or transformer extent.",
    purpose: "Reads the Rwanda UPI parcels (11.5 million in BigQuery, never installed nationally) that intersect exactly one bound: a village or cell, a rectangle of at most 25 km², a boundary (a corridor from `ds data vector buffer`, an admin unit) or one transformer's buffered design extent. Which parcels does this line's corridor cross: this command with the corridor as --boundary. Capped at 5,000 rows; the receipt states rows_total and whether the answer was truncated. Parcels carry UPI, parcel key, administrative names and area. To keep them with the project like building footprints: `ds data project-cache seed --dataset rwanda_upi_parcels`.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        VILLAGE_ARG,
        CELL_ARG,
        BBOX_ARG,
        BOUNDARY_ARG,
        TRANSFORMER_ARG,
        LIMIT_ARG,
        GEOMETRY_OUT_ARG,
        LANE_ARG,
    ],
    output: "Dataset (id, layer, source, residency cloud, version), the query bound, rows_cap/rows_returned/rows_total/truncated, counts per cell and the written path when --geometry-out was given.",
    examples: &[
        Example {
            command: "ds data vector buffer --layer mv_lines --distance-m 15 --out ./corridor.geojson && ds data parcels query --boundary ./corridor.geojson --geometry-out ./crossed-parcels.geojson",
            note: "Parcels crossed by a 15 m MV corridor, kept as a local layer.",
            runnable: false,
        },
        Example {
            command: "ds data parcels query --transformer fill_in_kabuhoro --output json",
            note: "Every parcel touching the transformer's buffered design extent, up to the cap.",
            runnable: false,
        },
    ],
    refusals: QUERY_REFUSALS,
    reference: Some("docs/reference/data.md"),
    search: &["parcel", "land", "plot", "corridor", "crossed", "cadastral", "foundation", "cloud", "bigquery"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn geometry_out(raw: &Path) -> Result<PathBuf, Failure> {
    let out = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| Failure::internal("output_refused", error.to_string()))?
            .join(raw)
    };
    if out.exists() {
        return Err(
            Failure::conflict("output_refused", "The --geometry-out file already exists.")
                .remedy(GEOMETRY_OUT_REFUSED.remedy),
        );
    }
    Ok(out)
}

fn write_geometry(out: &Path, features: &[Value]) -> Result<(), Failure> {
    let bytes = serde_json::to_vec(&json!({"type": "FeatureCollection", "features": features}))
        .map_err(|error| Failure::internal("output_refused", error.to_string()))?;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)
        .and_then(|mut file| file.write_all(&bytes))
        .map_err(|error| {
            Failure::conflict(
                "output_refused",
                format!("Could not write the geometry: {error}"),
            )
            .remedy(GEOMETRY_OUT_REFUSED.remedy)
        })
}

/// Bounds and vertex count of one GeoJSON geometry — evidence in place of
/// coordinates.
fn geometry_evidence(geometry: &Value) -> Value {
    let mut bounds = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    let mut positions = 0usize;
    fn walk(value: &Value, bounds: &mut [f64; 4], positions: &mut usize) {
        match value {
            Value::Array(items) => {
                if items.len() >= 2 && items[0].is_number() && items[1].is_number() {
                    let (x, y) = (
                        items[0].as_f64().unwrap_or(0.0),
                        items[1].as_f64().unwrap_or(0.0),
                    );
                    bounds[0] = bounds[0].min(x);
                    bounds[1] = bounds[1].min(y);
                    bounds[2] = bounds[2].max(x);
                    bounds[3] = bounds[3].max(y);
                    *positions += 1;
                } else {
                    items.iter().for_each(|item| walk(item, bounds, positions));
                }
            }
            _ => {}
        }
    }
    walk(&geometry["coordinates"], &mut bounds, &mut positions);
    json!({
        "type": geometry["type"],
        "bbox": if positions > 0 { json!(bounds) } else { Value::Null },
        "coordinate_positions": positions,
    })
}

/// The `Failure` the kernel-side validation of a foundation request maps to:
/// the request never left this machine, so the code is this command's own.
fn refused_locally(error: Failure) -> Failure {
    let message = error.message().to_owned();
    if message.contains("UPI") {
        Failure::invalid(UPI_INVALID.code, message).remedy(UPI_INVALID.remedy)
    } else if message.contains("exceeds") || message.contains("limit") {
        Failure::invalid(BOUND_EXCEEDED.code, message).remedy(BOUND_EXCEEDED.remedy)
    } else if message.contains("bound")
        || message.contains("code")
        || message.contains("rectangle")
    {
        Failure::invalid(INVALID_SCOPE.code, message).remedy(INVALID_SCOPE.remedy)
    } else {
        error
    }
}

fn read(lane: &str, request: &DataDistributionRequest) -> Result<Value, Failure> {
    match ds_cli_auth::data_distribution(lane, request) {
        Err(error) if error.code() == "auth_input_invalid" => Err(refused_locally(error)),
        result => result,
    }
}

pub fn run_lookup(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let request = DataDistributionRequest::UpiLookup {
        upi: inputs.require("upi")?.to_owned(),
    };
    request.validate().map_err(|error| {
        Failure::invalid(UPI_INVALID.code, error.to_string()).remedy(UPI_INVALID.remedy)
    })?;
    let out = inputs
        .value("geometry-out")
        .map(|value| geometry_out(Path::new(value)))
        .transpose()?;
    let answer = read(lane, &request)?;
    let feature = answer["feature"].clone();
    let mut receipt = json!({
        "dataset": answer["dataset"],
        "query": answer["query"],
        "rows_cap": answer["rows_cap"],
        "rows_returned": answer["rows_returned"],
        "rows_total": answer["rows_total"],
        "truncated": answer["truncated"],
        "parcel": feature["properties"],
        "geometry": geometry_evidence(&feature["geometry"]),
    });
    if let Some(out) = out {
        write_geometry(&out, std::slice::from_ref(&feature))?;
        receipt["geometry"]["written_to"] = json!(out.to_string_lossy());
    }
    Ok(receipt)
}

/// The rectangle one transformer's design covers on paper: its saved extent,
/// buffered by the project's own policy through the kernel's coverage plan —
/// the same plan a seed acquires with.
fn transformer_rectangle(lane: &str, name: &str) -> Result<([f64; 4], Value), Failure> {
    let context = ds_cli_auth::transformer_context(lane, name)?;
    let layers = serde_json::to_value(context.snapshot().layers()).unwrap_or(Value::Null);
    let extent = ds_project_data::extents::extent_of(name, &layers)
        .map_err(|error| Failure::invalid(INVALID_SCOPE.code, error.message().to_owned()))?;
    if extent.bounds.is_none() {
        return Err(Failure::conflict(
            NO_DESIGN_EXTENT.code,
            format!("{name} has no saved design extent to derive a rectangle from"),
        )
        .remedy(NO_DESIGN_EXTENT.remedy));
    }
    let policy = buffer_policy(&[])
        .map_err(|error| Failure::internal("project_dataset_store_failed", error))?;
    let scope = Scope {
        principal: context.identity().uid().to_owned(),
        project: context.snapshot().ds_project().to_owned(),
    };
    let dataset = policy::Dataset {
        id: "edcl_customers".into(),
        provider: "ds-brain:customers_query".into(),
        quality: policy::Quality::Source,
        parameters: Default::default(),
        source_version: String::new(),
        crs: "EPSG:4326".into(),
    };
    let plan = policy::plan(&scope, &dataset, &policy, std::slice::from_ref(&extent))
        .map_err(|error| Failure::internal("project_dataset_store_failed", error))?;
    let cluster = plan.clusters.first().ok_or_else(|| {
        Failure::conflict(
            NO_DESIGN_EXTENT.code,
            format!("{name} has no saved design extent to derive a rectangle from"),
        )
        .remedy(NO_DESIGN_EXTENT.remedy)
    })?;
    Ok((
        cluster.envelope,
        json!({
            "transformer": name,
            "buffer_m": cluster.buffer_m,
            "bbox": cluster.envelope,
        }),
    ))
}

fn parse_bbox(raw: &str) -> Result<[f64; 4], Failure> {
    let parts: Vec<f64> = raw
        .split(',')
        .map(|part| part.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .map_err(|_| {
            Failure::invalid(INVALID_SCOPE.code, "--bbox is west,south,east,north")
                .remedy(INVALID_SCOPE.remedy)
        })?;
    let bounds: [f64; 4] = parts.try_into().map_err(|_| {
        Failure::invalid(INVALID_SCOPE.code, "--bbox is west,south,east,north")
            .remedy(INVALID_SCOPE.remedy)
    })?;
    Ok(bounds)
}

/// One bound, read from the inputs: the same grammar for customers and parcels.
fn bound(inputs: &Inputs, lane: &str) -> Result<(BoundFields, Value), Failure> {
    let limit = match inputs.value("limit") {
        None => None,
        Some(raw) => Some(raw.trim().parse::<u64>().map_err(|_| {
            Failure::invalid(BOUND_EXCEEDED.code, "--limit is 1 to 5000").remedy(BOUND_EXCEEDED.remedy)
        })?),
    };
    let named = ["village", "cell", "bbox", "boundary", "transformer"]
        .iter()
        .filter(|name| inputs.value(name).is_some())
        .count();
    if named != 1 {
        return Err(Failure::invalid(
            INVALID_SCOPE.code,
            "name exactly one of --village, --cell, --bbox, --boundary or --transformer",
        )
        .remedy(INVALID_SCOPE.remedy));
    }
    let mut fields = BoundFields { limit, ..Default::default() };
    let mut scope = Value::Null;
    if let Some(code) = inputs.value("village") {
        fields.village_code = Some(code.trim().to_owned());
    } else if let Some(code) = inputs.value("cell") {
        fields.cell_code = Some(code.trim().to_owned());
    } else if let Some(raw) = inputs.value("bbox") {
        fields.bbox = Some(parse_bbox(raw)?);
    } else if let Some(path) = inputs.value("boundary") {
        fields.boundary = Some(read_boundary(Path::new(path))?);
    } else {
        let (bbox, described) = transformer_rectangle(lane, inputs.require("transformer")?)?;
        scope = described;
        fields.bbox = Some(bbox);
    }
    Ok((fields, scope))
}

#[derive(Default)]
struct BoundFields {
    village_code: Option<String>,
    cell_code: Option<String>,
    bbox: Option<[f64; 4]>,
    boundary: Option<Value>,
    limit: Option<u64>,
}

/// The polygon in a GeoJSON file: bare geometry, a Feature, or a
/// FeatureCollection holding exactly one feature. Validation of the shape
/// and its envelope is the client core's, shared with every other caller.
fn read_boundary(path: &Path) -> Result<Value, Failure> {
    let raw = std::fs::read_to_string(path).map_err(|error| {
        Failure::invalid(INVALID_SCOPE.code, format!("--boundary: cannot read {}: {error}", path.display()))
            .remedy(INVALID_SCOPE.remedy)
    })?;
    let value: Value = serde_json::from_str(&raw).map_err(|error| {
        Failure::invalid(INVALID_SCOPE.code, format!("--boundary: not GeoJSON: {error}"))
            .remedy(INVALID_SCOPE.remedy)
    })?;
    let geometry = match value["type"].as_str() {
        Some("FeatureCollection") => {
            let features = value["features"].as_array().cloned().unwrap_or_default();
            if features.len() != 1 {
                return Err(Failure::invalid(
                    INVALID_SCOPE.code,
                    format!("--boundary: the collection holds {} features; a bound is one polygon", features.len()),
                )
                .remedy(INVALID_SCOPE.remedy));
            }
            features[0]["geometry"].clone()
        }
        Some("Feature") => value["geometry"].clone(),
        Some("Polygon") | Some("MultiPolygon") => value,
        other => {
            return Err(Failure::invalid(
                INVALID_SCOPE.code,
                format!("--boundary: {} is not a Polygon or MultiPolygon", other.unwrap_or("this")),
            )
            .remedy(INVALID_SCOPE.remedy));
        }
    };
    Ok(geometry)
}

pub fn run_query(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let (fields, scope) = bound(inputs, lane)?;
    let request = DataDistributionRequest::CustomersQuery {
        village_code: fields.village_code,
        cell_code: fields.cell_code,
        bbox: fields.bbox,
        boundary: fields.boundary,
        limit: fields.limit,
    };
    let (answer, features, mut receipt) = answer(lane, &request, scope, inputs)?;
    let mut by_village: std::collections::BTreeMap<String, u64> = Default::default();
    let mut by_segment: std::collections::BTreeMap<String, u64> = Default::default();
    for feature in &features {
        let properties = &feature["properties"];
        let village = format!(
            "{} / {}",
            properties["cell"].as_str().unwrap_or("?"),
            properties["village"].as_str().unwrap_or("?")
        );
        *by_village.entry(village).or_default() += 1;
        let segment = properties["customer_segmentation"]
            .as_str()
            .unwrap_or("unknown")
            .to_owned();
        *by_segment.entry(segment).or_default() += 1;
    }
    receipt["by_village"] = json!(by_village);
    receipt["by_segmentation"] = json!(by_segment);
    drop(answer);
    Ok(receipt)
}

pub fn run_parcels(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let (fields, scope) = bound(inputs, lane)?;
    let request = DataDistributionRequest::ParcelsQuery {
        village_code: fields.village_code,
        cell_code: fields.cell_code,
        bbox: fields.bbox,
        boundary: fields.boundary,
        limit: fields.limit,
    };
    let (answer, features, mut receipt) = answer(lane, &request, scope, inputs)?;
    let mut by_cell: std::collections::BTreeMap<String, u64> = Default::default();
    let mut area = 0.;
    for feature in &features {
        let properties = &feature["properties"];
        let cell = format!(
            "{} / {}",
            properties["sector"].as_str().unwrap_or("?"),
            properties["cell"].as_str().unwrap_or("?")
        );
        *by_cell.entry(cell).or_default() += 1;
        area += properties["source_area"].as_f64().unwrap_or(0.);
    }
    receipt["by_cell"] = json!(by_cell);
    receipt["source_area_m2"] = json!(area.round());
    drop(answer);
    Ok(receipt)
}

/// Validate, read and shape the common receipt; write the geometry when asked.
fn answer(
    lane: &str,
    request: &DataDistributionRequest,
    scope: Value,
    inputs: &Inputs,
) -> Result<(Value, Vec<Value>, Value), Failure> {
    request.validate().map_err(|error| {
        refused_locally(Failure::invalid("auth_input_invalid", error.to_string()))
    })?;
    let out = inputs
        .value("geometry-out")
        .map(|value| geometry_out(Path::new(value)))
        .transpose()?;
    let answer = read(lane, request)?;
    let features = answer["features"].as_array().cloned().unwrap_or_default();
    let mut query = answer["query"].clone();
    if !scope.is_null() {
        query["scope"] = scope;
    }
    let mut receipt = json!({
        "dataset": answer["dataset"],
        "query": query,
        "rows_cap": answer["rows_cap"],
        "rows_returned": answer["rows_returned"],
        "rows_total": answer["rows_total"],
        "truncated": answer["truncated"],
    });
    if let Some(out) = out {
        write_geometry(&out, &features)?;
        receipt["geometry"] = json!({"written_to": out.to_string_lossy(), "features": features.len()});
    }
    Ok((answer, features, receipt))
}

pub fn render_lookup(data: &Value) -> String {
    let parcel = &data["parcel"];
    let geometry = &data["geometry"];
    let mut out = format!(
        "UPI {} · parcel {} · {} / {} / {} / {}\n  {} · {} coordinate positions · {} m² · {} ({}, {})\n",
        parcel["upi"].as_str().unwrap_or("?"),
        parcel["parcel_key"].as_str().unwrap_or("?"),
        parcel["province"].as_str().unwrap_or("?"),
        parcel["district"].as_str().unwrap_or("?"),
        parcel["sector"].as_str().unwrap_or("?"),
        parcel["cell"].as_str().unwrap_or("?"),
        geometry["type"].as_str().unwrap_or("geometry"),
        geometry["coordinate_positions"].as_u64().unwrap_or(0),
        parcel["source_area"].as_f64().map(|a| format!("{a:.0}")).unwrap_or_else(|| "?".into()),
        data["dataset"]["layer"].as_str().unwrap_or("?"),
        data["dataset"]["residency"].as_str().unwrap_or("?"),
        data["dataset"]["source"].as_str().unwrap_or("?"),
    );
    if let Some(path) = geometry["written_to"].as_str() {
        out.push_str(&format!("  geometry written to {path}\n"));
    }
    out
}

pub fn render_query(data: &Value) -> String {
    let mut out = format!(
        "{} of {} customers{} · {} ({}, {})\n",
        data["rows_returned"].as_u64().unwrap_or(0),
        data["rows_total"].as_u64().unwrap_or(0),
        if data["truncated"] == Value::Bool(true) {
            " · truncated at the cap"
        } else {
            ""
        },
        data["dataset"]["layer"].as_str().unwrap_or("?"),
        data["dataset"]["residency"].as_str().unwrap_or("?"),
        serde_json::to_string(&data["query"]["bound"]).unwrap_or_default(),
    );
    if let Some(map) = data["by_segmentation"].as_object() {
        for (segment, count) in map {
            out.push_str(&format!("  {:<14} {}\n", segment, count));
        }
    }
    if let Some(map) = data["by_village"].as_object() {
        for (village, count) in map {
            out.push_str(&format!("  {:<30} {}\n", village, count));
        }
    }
    if let Some(path) = data["geometry"]["written_to"].as_str() {
        out.push_str(&format!("  geometry written to {path}\n"));
    }
    out
}

pub fn render_parcels(data: &Value) -> String {
    let mut out = format!(
        "{} of {} parcels{} · {} m² · {} ({}, {})\n",
        data["rows_returned"].as_u64().unwrap_or(0),
        data["rows_total"].as_u64().unwrap_or(0),
        if data["truncated"] == Value::Bool(true) {
            " · truncated at the cap"
        } else {
            ""
        },
        data["source_area_m2"].as_f64().map(|a| format!("{a:.0}")).unwrap_or_else(|| "?".into()),
        data["dataset"]["layer"].as_str().unwrap_or("?"),
        data["dataset"]["residency"].as_str().unwrap_or("?"),
        serde_json::to_string(&data["query"]["bound"]).unwrap_or_default(),
    );
    if let Some(map) = data["by_cell"].as_object() {
        for (cell, count) in map {
            out.push_str(&format!("  {:<30} {}\n", cell, count));
        }
    }
    if let Some(path) = data["geometry"]["written_to"].as_str() {
        out.push_str(&format!("  geometry written to {path}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_counts_positions_and_bounds() {
        let polygon = json!({"type":"Polygon","coordinates":[[[29.5,-2.5],[29.6,-2.5],[29.6,-2.4],[29.5,-2.5]]]});
        let evidence = geometry_evidence(&polygon);
        assert_eq!(evidence["coordinate_positions"], 4);
        assert_eq!(evidence["bbox"], json!([29.5, -2.5, 29.6, -2.4]));
        let point = json!({"type":"Point","coordinates":[29.5,-2.5]});
        assert_eq!(geometry_evidence(&point)["coordinate_positions"], 1);
    }

    #[test]
    fn a_rectangle_is_four_ordered_numbers() {
        assert_eq!(parse_bbox("29.57,-2.53, 29.59,-2.51").unwrap(), [29.57, -2.53, 29.59, -2.51]);
        assert!(parse_bbox("29.57,-2.53").is_err());
        assert!(parse_bbox("a,b,c,d").is_err());
    }

    #[test]
    fn local_refusals_carry_this_commands_codes() {
        assert_eq!(
            refused_locally(Failure::invalid("auth_input_invalid", "A UPI is 8 to 20 digits")).code(),
            "upi_invalid"
        );
        assert_eq!(
            refused_locally(Failure::invalid("auth_input_invalid", "The rectangle exceeds the 25 km² bound")).code(),
            "bound_exceeded"
        );
        assert_eq!(
            refused_locally(Failure::invalid("auth_input_invalid", "A village code is exactly 8 digits")).code(),
            "invalid_admin_scope"
        );
    }
}
