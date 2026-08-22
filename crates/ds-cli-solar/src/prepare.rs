//! `ds solar prepare` — resolve weather and commit prepared inputs.
//!
//! This is the phase that may reach the network, and it says so in its
//! contract. Weather is resolved cache-first; without a `--weather-url` the
//! frozen fixture datasets are the provider and preparation is fully offline.
//!
//! The weather bearer token is never a flag. `ds-solar` reads it from
//! `DS_SOLAR_WEATHER_TOKEN` with `hide_env_values`, and `ds` passes the
//! environment through untouched rather than accepting a credential as an
//! argument — an argument would land in shell history, process listings and
//! an agent's context.

use std::ffi::OsString;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DS_SOLAR, PREPARE_TIMEOUT};

pub static COMMAND: Command = Command {
    id: "solar.prepare",
    path: &["solar", "prepare"],
    contract: 1,
    summary: "Resolve weather and commit prepared solar inputs.",
    purpose: "\
Prepares cities for a batch: resolves weather cache-first and writes prepared \
inputs a later `ds solar run` can execute offline. This is the only solar \
phase permitted to reach the network, and it does so only on a cache miss and \
only when --weather-url is given; without it the frozen fixture datasets are \
the provider. Supply the weather bearer token through DS_SOLAR_WEATHER_TOKEN \
in the environment — there is deliberately no flag for it.",
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
            "weather-cache",
            "<dir>",
            "Versioned weather cache directory.",
        ),
        Arg::value(
            "weather-url",
            "<url>",
            "ds-solar-weather base URL, consulted only on a cache miss. Omit to stay offline.",
        ),
        Arg::value(
            "project-id",
            "<id>",
            "Project id recorded in prepared inputs.",
        )
        .default("ds-solar-fixtures"),
        Arg::value(
            "root",
            "<name>",
            "Root collection recorded in prepared inputs.",
        )
        .default("solar"),
    ],
    output: "The output directory, the cities selected, whether the network was permitted, and the engine's summary.",
    examples: &[
        Example {
            command: "ds solar prepare --out ./prepared --output json",
            note: "Offline: no --weather-url, so the frozen fixture datasets are the provider.",
            runnable: false,
        },
        Example {
            command: "ds solar prepare --out ./prepared --city kigali --weather-cache ./wx --output json",
            note: "Cache-first, still offline.",
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
            code: "engine_refused",
            when: "the engine ran and failed, for example a weather fetch it could not complete",
            remedy: "read detail.engine for the engine's own message",
        },
        Refusal {
            code: "callee_timed_out",
            when: "preparation exceeded the thirty-minute bound",
            remedy: "prepare fewer cities, or warm the weather cache first",
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
    let weather_url = inputs.value("weather-url");

    let mut args: Vec<OsString> = vec![OsString::from("--out"), OsString::from(out)];
    for (flag, value) in [
        ("--project-id", inputs.value("project-id")),
        ("--root", inputs.value("root")),
        ("--weather-cache", inputs.value("weather-cache")),
        ("--weather-url", weather_url),
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
        // Stated explicitly so a caller auditing a prepared set can tell,
        // from the receipt alone, whether it could have left the machine.
        "network_permitted": weather_url.is_some(),
        "engine": completed
            .stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .take(20)
            .map(|line| line.chars().take(200).collect::<String>())
            .collect::<Vec<_>>(),
    }))
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "out      {}\ncities   {}\nnetwork  {}\n\n",
        data["out"].as_str().unwrap_or(""),
        data["cities"],
        if data["network_permitted"].as_bool().unwrap_or(false) {
            "permitted (--weather-url given)"
        } else {
            "not permitted"
        },
    );
    for line in data["engine"].as_array().into_iter().flatten() {
        out.push_str(&format!("{}\n", line.as_str().unwrap_or("")));
    }
    out
}
