//! `ds pls terrain-reconcile` — reconcile a terrain batch to surveyed ground.

use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_grid_tasks::{ReconcilePlsTerrainRequest, reconcile_pls_terrain};
use serde_json::{Value, json};

use crate::{output_path, source_path, workspace_path};

pub static COMMAND: Command = Command {
    id: "pls.terrain-reconcile",
    path: &["pls", "terrain-reconcile"],
    contract: 1,
    summary: "Correct a terrain-datum waterfall and taper surveyed route seams.",
    purpose: "Reads one closed baseline workspace, an explicit JSON/GeoJSON XYZ point batch, and ordered LineString routes. It derives bounded surveyed-TIN pairs, a robust median delta, and only those endpoint seams supported by nearby surveyed ground. XY, the source workspace, and free ends without authority remain unchanged. Dry-run writes nothing; commit creates one new workspace.",
    chapter: Chapter::PlsCadd,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("workspace", "<dir>", "Closed untouched PLS-CADD workspace.").required(),
        Arg::value(
            "points",
            "<json>",
            "Incoming XYZ point batch as JSON or Point GeoJSON.",
        )
        .required(),
        Arg::value(
            "routes",
            "<json>",
            "Ordered routes as JSON or LineString GeoJSON.",
        )
        .required(),
        Arg::value(
            "horizontal-crs",
            "<authority>",
            "Authoritative horizontal CRS declaration.",
        )
        .required(),
        Arg::value(
            "vertical-datum",
            "<authority>",
            "Declared vertical datum authority.",
        )
        .required(),
        Arg::value("out", "<new-dir>", "New absent workspace path.").required(),
        Arg::switch("dry-run", "Compute and verify the receipt without writing."),
    ],
    output: "Pair count and residual distribution, global delta, every resolved seam and blend, raw/workspace/output digests, point counts, zero XY-change count, unresolved free ends, persistence state, and the remaining engineer decision.",
    examples: &[
        Example {
            command: "ds pls terrain-reconcile --workspace ./baseline --points ./points.json --routes ./routes.geojson --horizontal-crs 'EDCL Rwanda TM' --vertical-datum 'project surveyed TIN' --out ./reconciled --dry-run --output json",
            note: "Derive and verify the exact correction without writing.",
            runnable: false,
        },
        Example {
            command: "ds pls terrain-reconcile --workspace ./baseline --points ./points.json --routes ./routes.geojson --horizontal-crs 'EDCL Rwanda TM' --vertical-datum 'project surveyed TIN' --out ./reconciled --yes --output json",
            note: "Atomically create one corrected workspace after reviewing the dry-run.",
            runnable: false,
        },
    ],
    refusals: &[
        Refusal {
            code: "confirmation_required",
            when: "neither --dry-run nor --yes was supplied",
            remedy: "run --dry-run first; then repeat the same invocation with --yes",
        },
        Refusal {
            code: "mode_conflict",
            when: "--dry-run and --yes were both supplied",
            remedy: "choose exactly one mode",
        },
        Refusal {
            code: "workspace_not_found",
            when: "--workspace is not a directory",
            remedy: "pass the closed baseline workspace root",
        },
        Refusal {
            code: "source_not_found",
            when: "--points or --routes is not a file",
            remedy: "check the exact evidence path",
        },
        Refusal {
            code: "output_exists",
            when: "--out already exists",
            remedy: "choose a new immutable output path",
        },
        Refusal {
            code: "workspace_open",
            when: "PLS-CADD or an explicit workspace marker holds the source open",
            remedy: "close PLS-CADD and retry against the unchanged baseline",
        },
        Refusal {
            code: "datum_authority_ambiguous",
            when: "the CRS or vertical datum is empty, unknown, assumed or auto",
            remedy: "supply the authoritative CRS and declared vertical datum",
        },
        Refusal {
            code: "ground_evidence_insufficient",
            when: "ground coverage, pair count, residual consistency or a safe seam is unsupported",
            remedy: "acquire authoritative surveyed ground or revise the bounded route evidence; do not force one offset",
        },
        Refusal {
            code: "route_evidence_invalid",
            when: "points/routes JSON is invalid or routes are empty, repeated, multipart or unordered",
            remedy: "supply finite XYZ rows and uniquely named ordered LineStrings",
        },
        Refusal {
            code: "output_write_failed",
            when: "a new staging/output workspace cannot be written or promoted",
            remedy: "choose a writable absent sibling path and inspect any named staging directory",
        },
        Refusal {
            code: "native_readback_failed",
            when: "the XYZ writer or exact point-count readback refuses",
            remedy: "keep the baseline unchanged and report the source/output digests",
        },
        crate::RESULT_ENCODING_REFUSAL,
    ],
    reference: Some("docs/reference/pls.md"),
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let commit = mode(inputs, context)?;
    let request = ReconcilePlsTerrainRequest {
        workspace_root: workspace_path(inputs.require("workspace")?)?,
        points_path: source_path(inputs.require("points")?, "points")?,
        routes_path: source_path(inputs.require("routes")?, "routes")?,
        horizontal_crs: inputs.require("horizontal-crs")?.to_string(),
        vertical_datum: inputs.require("vertical-datum")?.to_string(),
        output_root: output_path(inputs.require("out")?)?,
        commit,
    };
    let result = reconcile_pls_terrain(&request).map_err(map_task_error)?;
    serde_json::to_value(result)
        .map_err(|error| Failure::internal("result_unserializable", error.to_string()))
}

fn mode(inputs: &Inputs, context: &Context) -> Result<bool, Failure> {
    match (inputs.switch("dry-run"), context.confirmed) {
        (true, false) => Ok(false),
        (false, true) => Ok(true),
        (true, true) => Err(Failure::invalid(
            "mode_conflict",
            "--dry-run and --yes cannot be combined",
        )
        .remedy("choose exactly one mode")),
        (false, false) => Err(Failure::invalid(
            "confirmation_required",
            "choose a non-writing dry run or confirm the new workspace",
        )
        .remedy("run with --dry-run first; then repeat with --yes")),
    }
}

fn map_task_error(error: ds_grid_tasks::PlsTerrainTaskError) -> Failure {
    let (code, remedy) = match error.code.as_str() {
        "workspace_open" => (
            "workspace_open",
            "close PLS-CADD and retry against the unchanged source",
        ),
        "horizontal_crs_ambiguous" | "vertical_datum_ambiguous" => (
            "datum_authority_ambiguous",
            "supply the authoritative horizontal CRS and vertical datum",
        ),
        "missing_ground_coverage"
        | "ambiguous_ground_coverage"
        | "insufficient_pairs"
        | "datum_residuals_change_sign"
        | "datum_evidence_inconsistent"
        | "seam_verification_failed"
        | "seam_route_too_short"
        | "seam_overlap" => (
            "ground_evidence_insufficient",
            "acquire authoritative surveyed ground or revise the bounded route evidence",
        ),
        "points_invalid"
        | "routes_invalid"
        | "empty_point_batch"
        | "empty_route_set"
        | "invalid_point"
        | "unordered_route_ambiguity"
        | "evidence_not_found"
        | "evidence_not_file"
        | "evidence_read_failed" => (
            "route_evidence_invalid",
            "supply finite XYZ rows and uniquely named ordered LineStrings",
        ),
        "output_exists" => ("output_exists", "choose a new immutable output path"),
        "output_write_failed"
        | "output_parent_missing"
        | "output_publish_failed"
        | "staging_exists" => (
            "output_write_failed",
            "choose a writable absent sibling path and inspect any named staging directory",
        ),
        "native_write_refused" | "native_readback_mismatch" => (
            "native_readback_failed",
            "keep the baseline unchanged and report the source/output digests",
        ),
        _ => (
            "route_evidence_invalid",
            "read detail['task-code'], correct the bounded request, and retry once",
        ),
    };
    Failure::failed(code, error.detail)
        .remedy(remedy)
        .detail(json!({ "task-code": error.code }))
}

pub fn render(data: &Value) -> String {
    format!(
        "terrain reconciliation\n  pairs {} · delta {} m · seams {} · unresolved ends {}\n  points {} -> {} · XY changes {}\n  persisted {} · {}\n",
        data["residual_distribution"]["pair_count"],
        data["global_delta_m"],
        data["seams"].as_array().map(Vec::len).unwrap_or(0),
        data["unresolved_ends"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0),
        data["baseline_point_count"],
        data["output_point_count"],
        data["coordinate_change_count"],
        data["persisted"],
        data["output_root"].as_str().unwrap_or(""),
    )
}
