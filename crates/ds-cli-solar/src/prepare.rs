//! `ds solar prepare` — resolve reference bundles and commit prepared inputs.
//!
//! This is the phase that may reach the network, and it says so in its
//! contract. For each selected city it derives the bundle's identity from the
//! city's coordinates and the deployment's pin, reads the local cache, and
//! downloads only on a miss.
//!
//! **A cache hit is a complete skip.** A reference unit is a constant of its
//! coordinates and its pins — the same place under the same pvlib, the same
//! catalog rows and the same losses is the same 8,760 hours forever — so
//! re-downloading one because time passed would be paying for a round trip to
//! receive bytes already on disk. `--overwrite` is the deliberate refresh, and
//! it is the *only* way to pick up a producer-side pin change, because nothing
//! about a pvlib upgrade or a revised catalog row is visible from this side of
//! the boundary.
//!
//! ## Authority
//!
//! `ds` holds no credential and gains none here. When `--reference-url` names
//! an authenticated origin, the fetch is performed **by the paired
//! application**, under the identity it already holds, through one closed
//! bridge operation. The JWT is never fetched, never passed as an argument and
//! never held by this process — `ds` asks for an outcome, not for a token, and
//! `ds-cli-desktop`'s crate header explains why that distinction is the whole
//! security argument.
//!
//! Without a paired application, preparation is offline: the frozen fixture
//! bundles are the provider and a cache miss is a refusal with a remedy.

use std::ffi::OsString;
use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_cli_desktop::bridge;
use serde_json::{Value, json};

use crate::{DS_SOLAR, PREPARE_TIMEOUT};

/// The closed bridge operation the application answers with a downloaded,
/// verified, cached bundle set. Named here once; there is no second spelling.
const BRIDGE_FETCH: &str = "solar.reference.fetch";

/// Downloading seven cities' bundles is a minute of real work, and the
/// application performs the authenticated calls itself.
const BRIDGE_FETCH_TIMEOUT: Duration = Duration::from_secs(20 * 60);

pub static COMMAND: Command = Command {
    id: "solar.prepare",
    path: &["solar", "prepare"],
    contract: 1,
    summary: "Resolve reference bundles and commit prepared solar inputs.",
    purpose: "\
Prepares cities for a batch: resolves each city's sealed reference bundle — \
weather, the pinned pvlib equipment, the string design and the 8,760-hour \
reference-unit simulation — cache-first, and writes prepared inputs a later \
`ds solar run` can execute offline. This is the only solar phase permitted to \
reach the network, and it does so only on a cache miss and only when \
--reference-url is given; without it the frozen fixture bundles are the \
provider. A cached bundle is reused verbatim: a reference unit is a constant \
of its coordinates and pins, so refreshing one is --overwrite, never a \
time-to-live decision.",
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("out", "<dir>", "Where to write prepared inputs.").required(),
        Arg::repeated(
            "city",
            "<id>",
            "City context id. Repeat to select; omit to prepare all.",
        ),
        Arg::value(
            "cache",
            "<dir>",
            "Versioned reference-bundle cache directory.",
        ),
        Arg::value(
            "reference-url",
            "<url>",
            "ds-solar-weather base URL, consulted only on a cache miss. Omit to stay offline.",
        ),
        Arg::switch(
            "overwrite",
            "Re-download cached bundles. The only way to pick up a producer pin change.",
        ),
        Arg::value(
            "project",
            "<id>",
            "Project recorded in prepared inputs. Defaults to the paired session's selected project.",
        ),
        Arg::value(
            "root",
            "<name>",
            "Root collection recorded in prepared inputs.",
        )
        .default("solar"),
        Arg::value(
            "desktop-descriptor",
            "<path>",
            "Use this bridge descriptor instead of discovering one.",
        ),
    ],
    output: "\
The output directory, the cities selected, the project the inputs were \
committed under and where it came from, whether the network was permitted, and \
per city whether its bundle came from the cache or the provider.",
    examples: &[
        Example {
            command: "ds solar prepare --out ./prepared --output json",
            note: "Offline: no --reference-url, so the frozen fixture bundles are the provider.",
            runnable: false,
        },
        Example {
            command: "ds solar prepare --out ./prepared --city kigali --cache ./bundles --output json",
            note: "Cache-first, still offline. A hit downloads nothing.",
            runnable: false,
        },
        Example {
            command: "ds solar prepare --out ./prepared --cache ./bundles --reference-url https://api.example --overwrite --output json",
            note: "Re-downloads every selected city. Use after a producer pin change.",
            runnable: false,
        },
    ],
    refusals: &[
        Refusal {
            code: "solar_engine_missing",
            when: "`ds-solar` cannot be found; it is not a bundled sidecar",
            remedy: "set DS_SOLAR_BIN to a built ds-solar; see docs/reference/solar.md",
        },
        Refusal {
            code: "project_unknown",
            when: "no --project was given and no paired session has one selected",
            remedy: "pass --project <id>, or select a project in DS GridDesign",
        },
        Refusal {
            code: "cache_required",
            when: "--reference-url is given without --cache; downloaded bundles need somewhere to live",
            remedy: "pass --cache <dir>",
        },
        Refusal {
            code: "desktop_not_paired",
            when: "--reference-url needs an identity and no DS GridDesign session is running",
            remedy: "start DS GridDesign and sign in, then retry",
        },
        Refusal {
            code: "desktop_ambiguous",
            when: "more than one DS GridDesign session is running",
            remedy: "name one with --desktop-descriptor <path>",
        },
        Refusal {
            code: "desktop_unreachable",
            when: "the bridge descriptor names a session that does not answer",
            remedy: "DS GridDesign may have exited; restart it and retry",
        },
        Refusal {
            code: "desktop_unreadable",
            when: "the paired session's reply could not be read",
            remedy: "restart DS GridDesign and retry",
        },
        Refusal {
            code: "desktop_operation_unsupported",
            when: "this DS GridDesign build offers no reference-bundle fetch",
            remedy: "update DS GridDesign; `ds desktop status` reports the profile",
        },
        Refusal {
            code: "desktop_refused",
            when: "the paired session declined the fetch",
            remedy: "read detail for the application's own message",
        },
        Refusal {
            code: "desktop_contract_mismatch",
            when: "the paired session's reply does not match this build's contract",
            remedy: "update DS GridDesign and `ds` to matching releases",
        },
        Refusal {
            code: "pairing_rejected",
            when: "the descriptor's pairing secret is stale",
            remedy: "restart DS GridDesign",
        },
        Refusal {
            code: "engine_refused",
            when: "the engine ran and failed, for example a bundle it could not verify",
            remedy: "read detail.engine for the engine's own message",
        },
        Refusal {
            code: "callee_timed_out",
            when: "preparation exceeded the thirty-minute bound",
            remedy: "prepare fewer cities, or warm the cache first",
        },
    ],
    reference: Some("docs/reference/solar.md"),
    availability,
};

fn availability() -> Availability {
    DS_SOLAR.availability()
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let out = inputs.require("out")?;
    let cities = inputs.repeated("city");
    let reference_url = inputs.value("reference-url");
    let cache = inputs.value("cache");
    let overwrite = inputs.switch("overwrite");

    let (project, project_source) = resolve_project(inputs)?;

    // When the network is permitted, the *application* fetches. `ds` never
    // holds the identity that authorizes it, so this is not a convenience —
    // it is the only route that exists.
    let mut downloaded = Value::Null;
    if let Some(url) = reference_url {
        let Some(cache) = cache else {
            return Err(Failure::invalid(
                "cache_required",
                "--reference-url downloads bundles and needs --cache to put them in",
            )
            .remedy("pass --cache <dir>"));
        };
        let found = bridge::paired(inputs.value("desktop-descriptor"))?;
        downloaded = bridge::invoke(
            &found.descriptor,
            BRIDGE_FETCH,
            json!({
                "reference_url": url,
                "cache_dir": cache,
                "project": project,
                "root": inputs.value("root").unwrap_or("solar"),
                "city_ids": cities,
                "overwrite": overwrite,
            }),
            BRIDGE_FETCH_TIMEOUT,
        )?;
    }

    // The engine then prepares from the cache alone. It is handed no URL and
    // no token, so this half cannot reach the network whatever happened above.
    let mut args: Vec<OsString> = vec![OsString::from("--out"), OsString::from(out)];
    for (flag, value) in [
        ("--project-id", Some(project.as_str())),
        ("--root", inputs.value("root")),
        ("--cache", cache),
    ] {
        if let Some(value) = value {
            args.push(OsString::from(flag));
            args.push(OsString::from(value));
        }
    }
    for city in cities {
        args.push(OsString::from("--city"));
        args.push(OsString::from(city));
    }

    let completed = DS_SOLAR.call("prepare", &args, PREPARE_TIMEOUT)?;
    if !completed.succeeded() {
        return Err(DS_SOLAR.failure_from(&completed, "prepare"));
    }

    Ok(json!({
        "out": out,
        "cities": if cities.is_empty() { json!("all") } else { json!(cities) },
        "project": project,
        "project_source": project_source,
        "cache": cache,
        "overwrite": overwrite,
        // Stated explicitly so a caller auditing a prepared set can tell, from
        // the receipt alone, whether it could have left the machine.
        "network_permitted": reference_url.is_some(),
        "downloaded": downloaded,
        "engine": completed
            .stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .take(40)
            .map(|line| line.chars().take(200).collect::<String>())
            .collect::<Vec<_>>(),
    }))
}

/// Which project the prepared inputs are committed under, and where that came
/// from.
///
/// `--project` wins. Otherwise the paired application's selected project — the
/// same one its window is showing, so a terminal and a window never disagree
/// about what is being prepared. Neither available is a refusal with both
/// remedies, not a default: "default" is the wrong project to write into a
/// prepared input.
fn resolve_project(inputs: &Inputs) -> Result<(String, &'static str), Failure> {
    if let Some(project) = inputs.value("project") {
        return Ok((project.to_string(), "flag"));
    }
    let found = bridge::paired(inputs.value("desktop-descriptor")).map_err(|failure| {
        Failure::invalid(
            "project_unknown",
            "no --project was given and no paired session could be asked",
        )
        .remedy("pass --project <id>, or start DS GridDesign and select one")
        .detail(json!({ "pairing": failure.code() }))
    })?;
    let session = bridge::session(&found.descriptor)?;
    match session["project"]
        .as_str()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        Some(project) => Ok((project.to_string(), "paired session")),
        None => Err(Failure::invalid(
            "project_unknown",
            "the paired session has no project selected",
        )
        .remedy("pass --project <id>, or select a project in DS GridDesign")
        .next("ds desktop status")),
    }
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "out      {}\ncities   {}\nproject  {} ({})\ncache    {}\nnetwork  {}\n",
        data["out"].as_str().unwrap_or(""),
        data["cities"],
        data["project"].as_str().unwrap_or(""),
        data["project_source"].as_str().unwrap_or(""),
        data["cache"].as_str().unwrap_or("none"),
        if data["network_permitted"].as_bool().unwrap_or(false) {
            "permitted (--reference-url given)"
        } else {
            "not permitted"
        },
    );
    if let Some(fetched) = data["downloaded"]["cities"].as_array() {
        out.push_str(&format!("fetched  {}\n", fetched.len()));
    }
    out.push('\n');
    for line in data["engine"].as_array().into_iter().flatten() {
        out.push_str(&format!("{}\n", line.as_str().unwrap_or("")));
    }
    out
}
