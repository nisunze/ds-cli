//! File adaptation only. Native terrain owners plan, sample, reduce and export.

use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_terrain_host::SamplingFileError;
use serde_json::Value;

pub static DESCRIBE: Command = Command {
    id: "data.terrain.describe",
    path: &["data", "terrain", "describe"],
    contract: 1,
    summary: "Read terrain sampling settings, schemas and density semantics.",
    purpose: "Read the native request schema and defaults before sampling a route. Explains height-preserving point reduction, reproducible station randomness, engineering interval density, optional side widths, surface gaps and measured-versus-derived provenance. A single explicit source supplies a job; default side profiles are off. Settings are owned by the Rust sampler and schema is generated from its actual request types.",
    chapter: Chapter::Data,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[],
    output: "Generated strict request JSON schema, current sampling defaults, source choices, geometric and acquisition limits and the native interpretation of curve error and engineering density.",
    examples: &[Example {
        command: "ds data terrain describe --output json",
        note: "Discover the native request and adjustable defaults.",
        runnable: true,
    }],
    refusals: &[],
    reference: Some("docs/reference/data.md"),
    search: &["defaults", "tolerance", "reproducible"],
    requires: Requires::Server,
    availability: crate::available,
};

pub static SAMPLE: Command = Command {
    id: "data.terrain.sample",
    path: &["data", "terrain", "sample"],
    contract: 1,
    summary: "Sample terrain adaptively along a route with optional side profiles.",
    purpose: "Create representative terrain samples from one explicit source: governed Rwanda TIFF or a declared survey CSV surface. First discover data.terrain.describe, then supply its strict native request file. Rust evaluates a two-dimensional surface at actual route XY, retains genuine terrain changes, removes points only within the requested discrete baseline error, and enforces adjustable density limits tightened by supplied engineering intervals or weight spans. Seeded longitudinal randomness changes sampling positions, never survey observations or terrain heights. Side profiles are off by default; explicit signed offsets select their widths and side observation randomness stays separate from nominal profile cuts. Output samples are derived/interpolated and retain settings, methods, input digests and source coverage; no model edit, source mixing, offset or datum adjustment. Raster resolution, sampling spacing, source uncertainty and simplification error are distinct. Missing surface readings remain gaps; unsupported authored breaklines are refused. Results cannot certify continuous unsampled terrain or missing engineering context.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "request",
            "<file.json>",
            "Strict native request file; discover its schema and defaults with data terrain describe.",
        ).required(),
        Arg::value(
            "out",
            "<new-directory>",
            "Fresh directory for native sampled points, profiles and receipts; existing destinations are refused.",
        ).required(),
        Arg::switch(
            "dry-run",
            "Validate inputs and the query plan without provider acquisition, surface queries or output creation.",
        ),
    ],
    output: "Bounded native receipt: selected source and input digests, exact settings/seed, route/profile/sample counts, coverage gaps, discrete error and engineering density evidence, and authored file paths/digests. Complete sample arrays are in files, not the CLI envelope. Generated samples never claim to be measured observations.",
    examples: &[
        Example {
            command: "ds data terrain sample --request ./terrain-request.json --out ./terrain-preview --dry-run --output json",
            note: "Validate the native request and query budget before acquiring or writing terrain.",
            runnable: false,
        },
        Example {
            command: "ds data terrain sample --request ./terrain-request.json --out ./terrain-samples --output json",
            note: "Write the native sampled points and optional side profiles to a fresh directory.",
            runnable: false,
        },
    ],
    refusals: &[
        Refusal {
            code: "terrain_request_invalid",
            when: "The native request, route, CRS or settings are malformed or exceed algorithm limits",
            remedy: "Read data terrain describe and supply finite supported coordinates and bounded settings matching its strict schema.",
        },
        Refusal {
            code: "terrain_input_too_large",
            when: "An input file exceeds its byte or point admission bound",
            remedy: "Split the route or survey into bounded local jobs using the limits returned by data terrain describe.",
        },
        Refusal {
            code: "terrain_output_exists",
            when: "The output destination already exists",
            remedy: "Choose a fresh output directory; native terrain sampling never overwrites evidence.",
        },
        Refusal {
            code: "terrain_io",
            when: "Input files cannot be read or output files cannot be authored",
            remedy: "Check the input paths and output directory permissions.",
        },
        Refusal {
            code: "terrain_source_invalid",
            when: "Survey coordinates or source declarations cannot form an admitted surface",
            remedy: "Supply correctly declared survey XYZ in one CRS, resolve conflicting duplicate heights and choose a supported local surface extent.",
        },
        Refusal {
            code: "terrain_surface_failed",
            when: "A surface query cannot complete its bounded interpolation",
            remedy: "Read the native surface reason and reduce the query batch or repair the declared input surface.",
        },
        Refusal {
            code: "terrain_sampling_failed",
            when: "The native sampler or export cannot establish its requested result",
            remedy: "Keep the native refusal and input identities; review density/error limits through data terrain describe.",
        },
        Refusal {
            code: "terrain_breaklines_unsupported",
            when: "The request supplies authored breaklines requiring constrained triangulation",
            remedy: "Use a surface owner that supports the authored constraints; do not omit breaklines to force an unconstrained answer.",
        },
        Refusal {
            code: "terrain_provider_failed",
            when: "A fixed public Rwanda acquisition transport fails",
            remedy: "Check connectivity to the published Rwanda resource and retry the unchanged job after a transient failure.",
        },
        Refusal {
            code: "terrain_integrity_failed",
            when: "Pinned Rwanda metadata or exact TIFF ranges fail verification",
            remedy: "Preserve the refusal and verify the published Rwanda resource before retrying.",
        },
    ],
    reference: Some("docs/reference/data.md"),
    search: &["jitter", "thinning", "ground", "survey", "dem", "rwanda"],
    requires: Requires::Server,
    availability: crate::available,
};

pub fn run_describe(_inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    Ok(ds_terrain_host::describe())
}

pub fn run_sample(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request = inputs
        .value("request")
        .ok_or_else(|| Failure::invalid("terrain_request_invalid", "--request is required"))?;
    let out = inputs
        .value("out")
        .ok_or_else(|| Failure::invalid("terrain_request_invalid", "--out is required"))?;
    let receipt =
        ds_terrain_host::sample_file(Path::new(request), Path::new(out), inputs.switch("dry-run"))
            .map_err(failure)?;
    serde_json::to_value(receipt)
        .map_err(|error| Failure::internal("terrain_sampling_failed", error.to_string()))
}

fn failure(error: SamplingFileError) -> Failure {
    let failure = match error.code() {
        "terrain_request_invalid" => {
            Failure::invalid("terrain_request_invalid", "Invalid native terrain request")
        }
        "terrain_input_too_large" => Failure::invalid(
            "terrain_input_too_large",
            "Terrain input exceeds its admission bound",
        ),
        "terrain_output_exists" => Failure::conflict(
            "terrain_output_exists",
            "Terrain destination already exists",
        ),
        "terrain_io" => Failure::failed("terrain_io", "Terrain file I/O failed"),
        "terrain_source_invalid" => {
            Failure::invalid("terrain_source_invalid", "Terrain source is invalid")
        }
        "terrain_surface_failed" => {
            Failure::failed("terrain_surface_failed", "Terrain interpolation failed")
        }
        "terrain_breaklines_unsupported" => Failure::unavailable(
            "terrain_breaklines_unsupported",
            "Constrained terrain triangulation is unsupported",
        ),
        "terrain_provider_failed" => Failure::unavailable(
            "terrain_provider_failed",
            "Public Rwanda acquisition failed",
        ),
        "terrain_integrity_failed" => Failure::failed(
            "terrain_integrity_failed",
            "Rwanda acquisition integrity failed",
        ),
        _ => Failure::failed("terrain_sampling_failed", "Native terrain sampling failed"),
    }
    .with_message(error.to_string());
    match SAMPLE
        .refusals
        .iter()
        .find(|entry| entry.code == failure.code())
    {
        Some(entry) => failure.remedy(entry.remedy),
        None => failure,
    }
}

pub fn render(data: &Value) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(data).unwrap_or_default()
    )
}
