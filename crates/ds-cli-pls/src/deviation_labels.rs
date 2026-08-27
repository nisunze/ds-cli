//! `ds pls deviation-labels` — visible native terrain labels from ordered routes.

use ds_cli_contract::spec::{
    Arg, Authority, Availability, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_grid_tasks::{LabelPlsDeviationsRequest, label_pls_deviations};
use serde_json::{Value, json};

use crate::{output_path, source_path, workspace_path};

pub static COMMAND: Command = Command {
    id: "pls.deviation-labels",
    path: &["pls", "deviation-labels"],
    contract: 1,
    summary: "Label ordered deviation vertices, keeping surveyed endpoints intact.",
    purpose: "Derives first, internal and last vertices from uniquely named ordered LineStrings, matches them to the reconciled point-batch suffix, and changes only feature-code bytes. With endpoint preservation, an occupied T-Off, tap, transformer or other non-angle survey row remains untouched and the coincident batch row becomes the visible marker. Feature text is not alignment topology. Dry-run writes nothing; commit creates one new workspace.",
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "workspace",
            "<dir>",
            "Closed reconciled PLS-CADD workspace.",
        )
        .required(),
        Arg::value(
            "points",
            "<json>",
            "The same point batch used for reconciliation.",
        )
        .required(),
        Arg::value(
            "routes",
            "<json>",
            "Ordered routes as JSON or LineString GeoJSON.",
        )
        .required(),
        Arg::value(
            "internal-code",
            "<code>",
            "Feature code for internal vertices.",
        )
        .required(),
        Arg::value(
            "start-code",
            "<code>",
            "Feature code for ordered first vertices.",
        )
        .required(),
        Arg::value(
            "end-code",
            "<code>",
            "Feature code for ordered last vertices.",
        )
        .required(),
        Arg::switch(
            "preserve-occupied-endpoints",
            "Keep non-angle survey rows and label the coincident batch marker.",
        ),
        Arg::value("out", "<new-dir>", "New absent labelled workspace path.").required(),
        Arg::switch(
            "dry-run",
            "Compute and verify the label receipt without writing.",
        ),
    ],
    output: "Internal/start/end counts, preserved occupied survey rows, added coincident markers, exact changed fields, unchanged XYZ/flags verdicts, before/after digests, point counts, persistence state, and the remaining engineer decision.",
    examples: &[
        Example {
            command: "ds pls deviation-labels --workspace ./reconciled --points ./points.json --routes ./routes.geojson --internal-code angle-point-new --start-code deviation-start --end-code deviation-end --preserve-occupied-endpoints --out ./labelled --dry-run --output json",
            note: "Verify all route identities and the exact code-only write set.",
            runnable: false,
        },
        Example {
            command: "ds pls deviation-labels --workspace ./reconciled --points ./points.json --routes ./routes.geojson --internal-code angle-point-new --start-code deviation-start --end-code deviation-end --preserve-occupied-endpoints --out ./labelled --yes --output json",
            note: "Create one labelled workspace after reviewing the dry-run.",
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
            remedy: "pass the closed reconciled workspace root",
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
            remedy: "close PLS-CADD and retry against the unchanged reconciled workspace",
        },
        Refusal {
            code: "unordered_route_ambiguity",
            when: "routes are empty, repeated, multipart or not ordered LineStrings",
            remedy: "supply uniquely named ordered LineStrings",
        },
        Refusal {
            code: "unmatched_route_vertex",
            when: "a route vertex has zero or multiple batch points within tolerance",
            remedy: "correct the route/point evidence so each ordered vertex has one identity",
        },
        Refusal {
            code: "conflicting_start_end_identity",
            when: "one point has conflicting internal/start/end roles",
            remedy: "split or reorder the route evidence; do not guess one label",
        },
        Refusal {
            code: "occupied_endpoint_overwrite",
            when: "an endpoint has a preserved non-angle survey row but preservation mode is absent",
            remedy: "repeat with --preserve-occupied-endpoints to add a coincident marker",
        },
        Refusal {
            code: "point_batch_not_reconciled_suffix",
            when: "the workspace does not end with the same point batch XY in order",
            remedy: "run terrain-reconcile with this exact batch, then label that output",
        },
        Refusal {
            code: "evidence_invalid",
            when: "points/routes JSON or a label code is invalid",
            remedy: "supply finite XYZ rows, ordered LineStrings and printable feature codes",
        },
        Refusal {
            code: "output_write_failed",
            when: "a new staging/output workspace cannot be written or promoted",
            remedy: "choose a writable absent sibling path and inspect any named staging directory",
        },
        Refusal {
            code: "native_readback_failed",
            when: "the code-only XYZ write changes another field or cannot re-read",
            remedy: "keep the reconciled source unchanged and report both digests",
        },
        crate::RESULT_ENCODING_REFUSAL,
    ],
    reference: Some("docs/reference/pls.md"),
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let commit = mode(inputs, context)?;
    let request = LabelPlsDeviationsRequest {
        workspace_root: workspace_path(inputs.require("workspace")?)?,
        points_path: source_path(inputs.require("points")?, "points")?,
        routes_path: source_path(inputs.require("routes")?, "routes")?,
        internal_code: inputs.require("internal-code")?.to_string(),
        start_code: inputs.require("start-code")?.to_string(),
        end_code: inputs.require("end-code")?.to_string(),
        preserve_occupied_endpoints: inputs.switch("preserve-occupied-endpoints"),
        output_root: output_path(inputs.require("out")?)?,
        commit,
    };
    let result = label_pls_deviations(&request).map_err(map_task_error)?;
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
        "unordered_route_ambiguity" | "empty_route_set" => (
            "unordered_route_ambiguity",
            "supply uniquely named ordered LineStrings",
        ),
        "unmatched_route_vertex" | "ambiguous_route_vertex" => (
            "unmatched_route_vertex",
            "correct the route/point evidence so every vertex has one identity",
        ),
        "conflicting_start_end_identity" => (
            "conflicting_start_end_identity",
            "split or reorder the route evidence; do not guess one label",
        ),
        "occupied_endpoint_overwrite" => (
            "occupied_endpoint_overwrite",
            "repeat with --preserve-occupied-endpoints",
        ),
        "point_batch_not_reconciled_suffix" => (
            "point_batch_not_reconciled_suffix",
            "run terrain-reconcile with this exact batch, then label that output",
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
            "keep the reconciled source unchanged and report both digests",
        ),
        _ => (
            "evidence_invalid",
            "supply finite XYZ rows, ordered LineStrings and printable feature codes",
        ),
    };
    Failure::failed(code, error.detail)
        .remedy(remedy)
        .detail(json!({ "task-code": error.code }))
}

pub fn render(data: &Value) -> String {
    format!(
        "deviation labels\n  internal {} · starts {} · ends {}\n  preserved endpoint rows {} · added markers {}\n  points {} -> {} · persisted {}\n  {}\n",
        data["labels"]["internal_count"],
        data["labels"]["start_count"],
        data["labels"]["end_count"],
        data["labels"]["preserved_occupied_endpoint_rows"],
        data["labels"]["added_markers"],
        data["point_count_before"],
        data["point_count_after"],
        data["persisted"],
        data["output_root"].as_str().unwrap_or(""),
    )
}
