//! Headless public city vectors; geometry and acquisition belong to owners.
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_command_kernel::city_vectors::{self as policy, Bounds, MicrosoftTile};
use serde_json::Value;
use std::{io::Read, path::Path, time::Duration};
pub static COMMAND: Command = Command {
    id: "data.city-vectors",
    path: &["data", "city-vectors"],
    contract: 1,
    summary: "Seed city footprints from Microsoft and basemap vectors from OSM.",
    purpose: "Preview public Microsoft tile sizes and bounded OSM coverage, then use --write to acquire local GeoJSON for map composition. Supports Chad, Libya and other published countries. Roads, buildings, water, landuse and places form an OSM vector basemap without raster tile scraping. Microsoft supplies polygon footprints independently of Google coverage. No project design is changed; attribution and exact source/artifact hashes travel in the receipt.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("west", "<longitude>", "WGS84 western bound.").required(),
        Arg::value("south", "<latitude>", "WGS84 southern bound.").required(),
        Arg::value("east", "<longitude>", "WGS84 eastern bound.").required(),
        Arg::value("north", "<latitude>", "WGS84 northern bound.").required(),
        Arg::value(
            "country",
            "<name>",
            "Microsoft published country, such as Chad or Libya.",
        )
        .required(),
        Arg::value("source", "<both|microsoft|osm>", "Public vector source.")
            .default("both")
            .choices(&["both", "microsoft", "osm"]),
        Arg::value(
            "out",
            "<dir>",
            "Fresh output directory; preview creates nothing.",
        )
        .required(),
        Arg::switch(
            "write",
            "Download the previewed scope and write verified GeoJSON and receipt.",
        ),
    ],
    output: "Preview: bounds, selected Microsoft tiles with published sizes, license and provider fee. Write: source hashes, GeoJSON files/counts/hashes and attribution; no report or sync claim.",
    examples: &[],
    refusals: &[
        Refusal {
            code: "city_vectors_invalid",
            when: "bounds/source/country is invalid",
            remedy: "use an ordered city box within two degrees and a published country",
        },
        Refusal {
            code: "city_vectors_failed",
            when: "source, parsing, bounds or output verification failed",
            remedy: "read the named source cause; use a fresh output path and correct the scope",
        },
    ],
    reference: Some("docs/reference/data.md"),
    requires: Requires::Server,
    availability: || Availability::Available,
};
struct PublicFetch;
fn bytes(mut response: ureq::http::Response<ureq::Body>, limit: usize) -> Result<Vec<u8>, String> {
    if !response.status().is_success() {
        return Err(format!(
            "public vector provider answered {}",
            response.status()
        ));
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "public vector response interrupted")?;
    if bytes.len() > limit {
        return Err("public vector response exceeds download bound".into());
    }
    Ok(bytes)
}
fn retry_public(
    mut request: impl FnMut() -> Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<ureq::http::Response<ureq::Body>, String> {
    for attempt in 0..3 {
        let response = request().map_err(|_| "public vector provider unreachable")?;
        if matches!(response.status().as_u16(), 429 | 503) {
            if attempt == 2 {
                return Err("public vector provider throttled; retry after its cooldown".into());
            }
            let wait = response
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(60)
                .clamp(10, 120);
            drop(response);
            std::thread::sleep(Duration::from_secs(wait));
        } else {
            return Ok(response);
        }
    }
    unreachable!("bounded attempts return a response or refusal")
}
fn get(url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let response = retry_public(|| {
        ureq::get(url)
            .header(
                "User-Agent",
                "DataSolutions-city-vectors/1.0 (datasolutions.rw)",
            )
            .config()
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(120)))
            .http_status_as_error(false)
            .build()
            .call()
    })?;
    bytes(response, limit)
}
impl ds_project_data::city_vectors::Provider for PublicFetch {
    fn microsoft_index(&mut self) -> Result<Vec<u8>, String> {
        get(policy::MICROSOFT_INDEX, policy::INDEX_MAX_BYTES)
    }
    fn microsoft_tile(&mut self, tile: &MicrosoftTile) -> Result<Vec<u8>, String> {
        get(&tile.url, policy::SOURCE_MAX_BYTES)
    }
    fn osm(&mut self, query: &str) -> Result<Vec<u8>, String> {
        let response = retry_public(|| {
            ureq::post(policy::OSM_ENDPOINT)
                .header(
                    "User-Agent",
                    "DataSolutions-city-vectors/1.0 (datasolutions.rw)",
                )
                .header("Content-Type", "text/plain")
                .config()
                .max_redirects(0)
                .timeout_global(Some(Duration::from_secs(120)))
                .http_status_as_error(false)
                .build()
                .send(query.as_bytes())
        })?;
        bytes(response, policy::SOURCE_MAX_BYTES)
    }
}
pub fn run(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let number = |key: &str| -> Result<f64, Failure> {
        i.require(key)?.parse().map_err(|_| {
            Failure::invalid(
                "city_vectors_invalid",
                format!("{key} must be a WGS84 number"),
            )
            .remedy("pass decimal longitude/latitude")
        })
    };
    let bounds = Bounds {
        west: number("west")?,
        south: number("south")?,
        east: number("east")?,
        north: number("north")?,
    };
    ds_project_data::city_vectors::acquire(
        &mut PublicFetch,
        bounds,
        i.require("country")?,
        i.value("source").unwrap_or("both"),
        Path::new(i.require("out")?),
        i.switch("write"),
    )
    .map_err(|e| {
        let failure = if e.contains("throttled") || e.contains("unreachable") {
            Failure::unavailable("city_vectors_failed", e)
        } else {
            Failure::failed("city_vectors_failed", e)
        };
        failure.remedy(
            "correct the named scope/provider/output cause; no existing output is overwritten",
        )
    })
}
pub fn render(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}
