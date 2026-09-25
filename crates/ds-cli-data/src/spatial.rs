//! Version-pinned, planned geographic reads of cloud reference datasets.
//! Geometry and billing decisions remain with ds-brain; this module only
//! selects one catalogue row, preserves the exact plan, and writes the answer.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::{DataDistributionRequest, SpatialResult};
use serde_json::{Value, json};

const PROJECT: Arg = Arg::value(
    "project",
    "<exact-id>",
    "Project billed and authorized for this read.",
)
.required();
const DATASET: Arg = Arg::value(
    "dataset",
    "<layer|id>",
    "Exact catalogue layer or dataset id from `ds data project-cache status --summary`.",
)
.required();
const BOUNDARY: Arg = Arg::value(
    "boundary",
    "<polygon.geojson>",
    "One WGS84 Polygon or MultiPolygon, including a one-feature buffer output.",
)
.required();
const RESULT: Arg = Arg::value(
    "result",
    "<features|count>",
    "Return bounded GeoJSON features or an exact aggregate count.",
)
.default("features")
.choices(&["features", "count"]);
const GROUP_BY: Arg = Arg::value(
    "group-by",
    "<sector>",
    "For counts, group by exact Rwanda sector boundary geometry.",
)
.choices(&["sector"]);
const DISTINCT: Arg = Arg::value(
    "distinct",
    "<upi>",
    "For parcel counts, count distinct UPI; inferred for the parcels dataset.",
)
.choices(&["upi"]);
const LIMIT: Arg = Arg::value(
    "limit",
    "<1..5000>",
    "Maximum features; execution reports total and truncation.",
);
const LANE: Arg = Arg::value("lane", "<stable|canary>", "Native authentication lane.")
    .default("stable")
    .choices(&["stable", "canary"]);
const OUT: Arg = Arg::value(
    "out",
    "<new.json>",
    "Fresh JSON plan or result path; existing files are never overwritten.",
)
.required();
const PLAN: Arg = Arg::value(
    "plan",
    "<plan.json>",
    "Plan file written by `ds data spatial plan`; contains the exact geometry and source pin.",
)
.required();
const GEOMETRY_OUT: Arg = Arg::value(
    "geometry-out",
    "<new.geojson>",
    "For complete feature answers, also write a GeoJSON FeatureCollection for local layers.",
);

const INVALID: Refusal = Refusal {
    code: "spatial_plan_invalid",
    when: "The selected dataset, geometry, plan file or result options are invalid.",
    remedy: "Read the dataset status and plan command contract; use one exact cloud layer and a bounded Polygon/MultiPolygon.",
};
const SOURCE_CHANGED: Refusal = Refusal {
    code: "spatial_source_changed",
    when: "The dataset or sector authority changed after planning.",
    remedy: "Run `ds data spatial plan` again with the same bound.",
};
const PLAN_CHANGED: Refusal = Refusal {
    code: "spatial_plan_changed",
    when: "The plan hash or query options differ from the planned request.",
    remedy: "Execute the untouched plan file, or plan again.",
};
const INCOMPLETE: Refusal = Refusal {
    code: "spatial_result_incomplete",
    when: "A feature result exceeds its row cap and cannot form a complete GeoJSON local layer.",
    remedy: "Narrow or partition the boundary, plan again, and deduplicate features by stable identity.",
};
const OUTPUT: Refusal = Refusal {
    code: "output_refused",
    when: "An output exists or cannot be created or written.",
    remedy: "Choose fresh writable output paths.",
};
const UNAVAILABLE: Refusal = Refusal {
    code: "spatial_query_unavailable",
    when: "The governed spatial query route or dataset cannot answer.",
    remedy: "Check the dataset and deployment, then retry the same bounded request.",
};
const SPATIAL_REFUSAL_SET: [Refusal; 7 + super::foundation::AUTH_REFUSALS] =
    super::foundation::with_native_refusals::<7, { 7 + super::foundation::AUTH_REFUSALS }>([
        INVALID,
        SOURCE_CHANGED,
        PLAN_CHANGED,
        INCOMPLETE,
        OUTPUT,
        UNAVAILABLE,
        ds_cli_auth::DATA_DISTRIBUTION_UNAVAILABLE_REFUSAL,
    ]);
const SPATIAL_REFUSALS: &[Refusal] = &SPATIAL_REFUSAL_SET;

pub static PLAN_COMMAND: Command = Command {
    id: "data.spatial.plan",
    path: &["data", "spatial", "plan"],
    contract: 1,
    summary: "Plan a governed BigQuery geography read before paying for it.",
    purpose: "Select one project-visible catalogue geography and a bounded GeoJSON area, then dry-run the exact spatial query. The plan pins the dataset version, optional sector-boundary authority, geometry, result shape, estimated bytes, billing guard and hash. Execution requires this plan file; no SQL or table name is accepted.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT, DATASET, BOUNDARY, RESULT, GROUP_BY, DISTINCT, LIMIT, OUT, LANE,
    ],
    output: "Compact plan receipt and path; the file holds the exact geometry and query options for execution. Feature row completeness is unknown until execution; count aggregates are complete.",
    examples: &[Example {
        command: "ds data spatial plan --project <id> --dataset rwanda_upi_parcels --boundary ./corridor.geojson --result count --group-by sector --out ./parcel-plan.json --output json",
        note: "Dry-run a parcel corridor count by exact sector polygons.",
        runnable: false,
    }],
    refusals: SPATIAL_REFUSALS,
    reference: Some("docs/reference/data.md"),
    search: &[
        "bigquery",
        "geospatial",
        "dataset",
        "parcels",
        "customers",
        "corridor",
        "sector",
        "cost",
        "dry run",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static EXECUTE_COMMAND: Command = Command {
    id: "data.spatial.execute",
    path: &["data", "spatial", "execute"],
    contract: 1,
    summary: "Run an exact version-pinned spatial plan and keep its result.",
    purpose: "Read one untouched `data.spatial.plan` file, re-authorize its named project, and execute its exact bounded query under the plan's billing guard. Writes the full JSON result to a fresh file. Complete feature results can also be kept as GeoJSON for `ds map local register`; truncated feature answers never become a local layer. A sector count JSON result can become an Excel workbook through `ds report spatial workbook`.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[PROJECT, PLAN, OUT, GEOMETRY_OUT, LANE],
    output: "Dataset, authority, plan hash, result type, counts, truncation and written paths. The JSON result file carries the complete feature array or sector groups.",
    examples: &[Example {
        command: "ds data spatial execute --project <id> --plan ./parcel-plan.json --out ./parcel-counts.json --output json",
        note: "Execute precisely the planned parcel-sector aggregate.",
        runnable: false,
    }],
    refusals: SPATIAL_REFUSALS,
    reference: Some("docs/reference/data.md"),
    search: &[
        "bigquery",
        "geospatial",
        "dataset",
        "parcels",
        "customers",
        "corridor",
        "sector",
        "geojson",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn file(path: &Path) -> Result<File, Failure> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| {
            Failure::invalid(
                OUTPUT.code,
                format!("cannot create {}: {error}", path.display()),
            )
            .remedy(OUTPUT.remedy)
        })
}

fn write_json(path: &Path, value: &Value) -> Result<(), Failure> {
    let mut output = file(path)?;
    let result = serde_json::to_vec(value)
        .map_err(|error| error.to_string())
        .and_then(|bytes| {
            output
                .write_all(&bytes)
                .map_err(|error| error.to_string())?;
            output.write_all(b"\n").map_err(|error| error.to_string())?;
            output.flush().map_err(|error| error.to_string())
        });
    if let Err(error) = result {
        let _ = std::fs::remove_file(path);
        return Err(Failure::invalid(
            OUTPUT.code,
            format!("cannot write {}: {error}", path.display()),
        )
        .remedy(OUTPUT.remedy));
    }
    Ok(())
}

fn read_json(path: &Path) -> Result<Value, Failure> {
    let bytes = std::fs::read(path).map_err(|error| {
        Failure::invalid(
            INVALID.code,
            format!("cannot read {}: {error}", path.display()),
        )
        .remedy(INVALID.remedy)
    })?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(
            Failure::invalid(INVALID.code, "spatial plan file exceeds 4 MiB")
                .remedy(INVALID.remedy),
        );
    }
    serde_json::from_slice(&bytes).map_err(|error| {
        Failure::invalid(INVALID.code, format!("invalid spatial plan JSON: {error}"))
            .remedy(INVALID.remedy)
    })
}

fn result(raw: &str) -> Result<SpatialResult, Failure> {
    match raw {
        "features" => Ok(SpatialResult::Features),
        "count" => Ok(SpatialResult::Count),
        _ => Err(
            Failure::invalid(INVALID.code, "--result is features or count").remedy(INVALID.remedy),
        ),
    }
}

fn receipt_for_project<'a>(answer: &'a Value, project: &str) -> Result<&'a Value, Failure> {
    if answer["project_id"] != project {
        return Err(Failure::unavailable(
            UNAVAILABLE.code,
            "spatial receipt does not name the requested project",
        )
        .remedy(UNAVAILABLE.remedy));
    }
    Ok(answer)
}

pub fn run_plan(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let project = inputs.require("project")?;
    let dataset = inputs.require("dataset")?;
    let boundary = super::foundation::read_boundary(Path::new(inputs.require("boundary")?))?;
    let choice = result(inputs.require("result")?)?;
    let rows =
        ds_cli_auth::data_distribution(lane, project, &DataDistributionRequest::ListDatasets {})?;
    let catalogue = rows.as_array().ok_or_else(|| {
        Failure::unavailable(UNAVAILABLE.code, "dataset catalogue is unreadable")
            .remedy(UNAVAILABLE.remedy)
    })?;
    let matches: Vec<_> = catalogue
        .iter()
        .filter(|row| row["id"] == dataset || row["layer"] == dataset)
        .collect();
    if matches.len() != 1 {
        return Err(Failure::invalid(
            INVALID.code,
            format!("dataset {dataset:?} is not one exact project-visible layer or id"),
        )
        .remedy(INVALID.remedy));
    }
    let row = matches[0];
    let resource_id = row["id"].as_str().ok_or_else(|| {
        Failure::invalid(INVALID.code, "catalogue dataset has no id").remedy(INVALID.remedy)
    })?;
    let distinct_field = inputs.value("distinct").map(str::to_owned).or_else(|| {
        (choice == SpatialResult::Count && row["layer"] == "rwanda_upi_parcels")
            .then(|| "upi".to_owned())
    });
    let group_by = inputs.value("group-by").map(str::to_owned);
    let limit = inputs
        .value("limit")
        .map(|raw| {
            raw.parse::<u64>().map_err(|_| {
                Failure::invalid(INVALID.code, "--limit must be 1 through 5000")
                    .remedy(INVALID.remedy)
            })
        })
        .transpose()?;
    let request = DataDistributionRequest::SpatialPlan {
        resource_id: resource_id.to_owned(),
        boundary: boundary.clone(),
        result: choice,
        distinct_field: distinct_field.clone(),
        group_by: group_by.clone(),
        limit,
    };
    request.validate().map_err(|error| {
        Failure::invalid(INVALID.code, error.to_string()).remedy(INVALID.remedy)
    })?;
    let answer = ds_cli_auth::data_distribution(lane, project, &request)?;
    receipt_for_project(&answer, project)?;
    if answer["dataset"]["id"] != resource_id || answer["dataset"]["version"].as_str().is_none() {
        return Err(Failure::unavailable(
            UNAVAILABLE.code,
            "spatial plan omitted the selected dataset or source version",
        )
        .remedy(UNAVAILABLE.remedy));
    }
    let path = Path::new(inputs.require("out")?);
    let document = json!({"schema":"ds.spatial-plan/v1","project":project,"resource_id":resource_id,"resource_version":answer["dataset"]["version"],"boundary":boundary,"result":inputs.require("result")?,"distinct_field":distinct_field,"group_by":group_by,"limit":limit,"plan_hash":answer["plan_hash"],"receipt":answer});
    write_json(path, &document)?;
    Ok(
        json!({"written_to":path,"dataset":document["receipt"]["dataset"],"authority":document["receipt"]["authority"],"query":document["receipt"]["query"],"estimate":document["receipt"]["estimate"],"plan_hash":document["plan_hash"]}),
    )
}

pub fn run_execute(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    let lane = inputs.require("lane")?;
    let plan = read_json(Path::new(inputs.require("plan")?))?;
    if plan["schema"] != "ds.spatial-plan/v1" || plan["project"] != project {
        return Err(Failure::invalid(
            INVALID.code,
            "plan schema or exact project does not match this request",
        )
        .remedy(INVALID.remedy));
    }
    let required = |key: &str| {
        plan[key].as_str().map(str::to_owned).ok_or_else(|| {
            Failure::invalid(INVALID.code, format!("plan lacks {key}")).remedy(INVALID.remedy)
        })
    };
    let choice = result(&required("result")?)?;
    let request = DataDistributionRequest::SpatialExecute {
        resource_id: required("resource_id")?,
        resource_version: required("resource_version")?,
        boundary: plan["boundary"].clone(),
        result: choice,
        distinct_field: plan["distinct_field"].as_str().map(str::to_owned),
        group_by: plan["group_by"].as_str().map(str::to_owned),
        limit: plan["limit"].as_u64(),
        plan_hash: required("plan_hash")?,
    };
    request.validate().map_err(|error| {
        Failure::invalid(INVALID.code, error.to_string()).remedy(INVALID.remedy)
    })?;
    if choice == SpatialResult::Count && inputs.value("geometry-out").is_some() {
        return Err(
            Failure::invalid(INVALID.code, "count results have no GeoJSON geometry")
                .remedy(INVALID.remedy),
        );
    }
    let answer = ds_cli_auth::data_distribution(lane, project, &request)?;
    receipt_for_project(&answer, project)?;
    if answer["dataset"]["id"] != plan["resource_id"]
        || answer["dataset"]["version"] != plan["resource_version"]
        || answer["plan_hash"] != plan["plan_hash"]
    {
        return Err(Failure::unavailable(
            UNAVAILABLE.code,
            "spatial execution receipt differs from the pinned plan",
        )
        .remedy(UNAVAILABLE.remedy));
    }
    let out = Path::new(inputs.require("out")?);
    if inputs
        .value("geometry-out")
        .is_some_and(|path| Path::new(path) == out)
    {
        return Err(Failure::invalid(
            OUTPUT.code,
            "--out and --geometry-out must name different files",
        )
        .remedy(OUTPUT.remedy));
    }
    if out.exists()
        || inputs
            .value("geometry-out")
            .is_some_and(|path| Path::new(path).exists())
    {
        return Err(
            Failure::invalid(OUTPUT.code, "an output path already exists").remedy(OUTPUT.remedy),
        );
    }
    write_json(out, &answer)?;
    if choice == SpatialResult::Features {
        if answer["truncated"] == true && inputs.value("geometry-out").is_some() {
            return Err(Failure::invalid(
                INCOMPLETE.code,
                format!(
                    "{} holds a truncated result; no GeoJSON layer was written",
                    out.display()
                ),
            )
            .remedy(INCOMPLETE.remedy));
        }
        if let Some(path) = inputs.value("geometry-out") {
            let fc = json!({"type":"FeatureCollection","features":answer["features"]});
            write_json(Path::new(path), &fc)?;
        }
    }
    Ok(
        json!({"written_to":out,"geometry_out":inputs.value("geometry-out"),"dataset":answer["dataset"],"authority":answer["authority"],"query":answer["query"],"plan_hash":answer["plan_hash"],"rows_returned":answer["rows_returned"],"rows_total":answer["rows_total"],"truncated":answer["truncated"],"overall_count":answer["overall_count"],"groups":answer["groups"],"unassigned_count":answer["unassigned_count"]}),
    )
}

pub fn render_plan(value: &Value) -> String {
    format!(
        "Spatial plan {} · {}\n",
        value["plan_hash"].as_str().unwrap_or("?"),
        value["written_to"].as_str().unwrap_or("?")
    )
}
pub fn render_execute(value: &Value) -> String {
    format!(
        "Spatial result · {}\n",
        value["written_to"].as_str().unwrap_or("?")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spatial_receipt_from_another_project_is_refused() {
        let answer = json!({"project_id": "project-b"});
        assert!(receipt_for_project(&answer, "project-b").is_ok());
        let error = receipt_for_project(&answer, "project-a").unwrap_err();
        assert_eq!(error.code(), "spatial_query_unavailable");
    }
}
