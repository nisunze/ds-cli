//! `ds solar verify-weather` — re-derive a weather dataset's digest.

use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::local_artifact;
use ds_solar_contracts::{WeatherDataset, WeatherKey};

pub static COMMAND: Command = Command {
    id: "solar.verify-weather",
    path: &["solar", "verify-weather"],
    contract: 1,
    summary: "Re-derive and validate a weather dataset's content digest.",
    purpose: "\
Recomputes a weather dataset's canonical content digest and checks it against \
the one the dataset carries. Run it when a prepared batch is about to be \
trusted, or when two machines produce different results from what should be \
the same weather. It reads the file and writes nothing. The shared pure Solar \
owner validates it; no Solar executable or installed addon is required.",
    chapter: Chapter::Solar,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[Arg::value("dataset", "<path>", "The weather dataset to verify.").required()],
    output: "Whether the digest re-derives, and the shared Solar owner's report (retained in the engine field for contract compatibility).",
    examples: &[Example {
        command: "ds solar verify-weather --dataset ./wx/kigali.json --output json",
        note: "",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "dataset_not_found",
            when: "--dataset does not name a file",
            remedy: "check the path",
        },
        Refusal {
            code: "engine_refused",
            when: "the digest did not re-derive, or the dataset is unreadable, schema-incompatible or above 64 MiB",
            remedy: "read detail.engine; a digest mismatch means the bytes changed",
        },
    ],
    reference: Some("docs/reference/solar.md"),
    search: &[],
    requires: Requires::Server,
    availability,
};

fn availability() -> Availability {
    Availability::Available
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let dataset = inputs.require("dataset")?;
    if !Path::new(dataset).is_file() {
        return Err(
            Failure::invalid("dataset_not_found", format!("`{dataset}` is not a file"))
                .remedy("check the path"),
        );
    }

    let weather: WeatherDataset = local_artifact::read(Path::new(dataset), "verify-weather")?;
    weather
        .validate()
        .map_err(|error| local_artifact::refused("verify-weather", error))?;
    let key = WeatherKey::of_dataset(&weather)
        .map_err(|error| local_artifact::refused("verify-weather", error))?;
    let report = format!(
        "schema   {}\nkey      {}\nrecords  {}\ndigest   {}\nvalid    yes",
        weather.schema_version,
        key.cache_entry(),
        weather.record_count(),
        weather.content_digest
    );

    Ok(json!({
        "dataset": dataset,
        "verified": true,
        "engine": report,
    }))
}

pub fn render(data: &Value) -> String {
    format!(
        "verified  {}\n{}",
        data["dataset"].as_str().unwrap_or(""),
        data["engine"].as_str().unwrap_or(""),
    )
}
