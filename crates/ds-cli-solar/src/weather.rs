//! `ds solar verify-weather` — re-derive a weather dataset's digest.

use std::ffi::OsString;
use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DISCOVERY_TIMEOUT, DS_SOLAR};

pub static COMMAND: Command = Command {
    id: "solar.verify-weather",
    path: &["solar", "verify-weather"],
    contract: 1,
    summary: "Re-derive and validate a weather dataset's content digest.",
    purpose: "\
Recomputes a weather dataset's canonical content digest and checks it against \
the one the dataset carries. Run it when a prepared batch is about to be \
trusted, or when two machines produce different results from what should be \
the same weather. It reads the file and writes nothing.",
    chapter: Chapter::Solar,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[Arg::value("dataset", "<path>", "The weather dataset to verify.").required()],
    output: "Whether the digest re-derives, and the engine's own report.",
    examples: &[Example {
        command: "ds solar verify-weather --dataset ./wx/kigali.json --output json",
        note: "",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "solar_engine_missing",
            when: "the packaged `ds-solar` sidecar cannot be found",
            remedy: "reinstall the complete ds release, or set DS_SOLAR_BIN for development",
        },
        Refusal {
            code: "dataset_not_found",
            when: "--dataset does not name a file",
            remedy: "check the path",
        },
        Refusal {
            code: "engine_refused",
            when: "the digest did not re-derive, or the dataset is unreadable",
            remedy: "read detail.engine; a digest mismatch means the bytes changed",
        },
    ],
    reference: Some("docs/reference/solar.md"),
    availability,
};

fn availability() -> Availability {
    DS_SOLAR.availability()
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let dataset = inputs.require("dataset")?;
    if !Path::new(dataset).is_file() {
        return Err(
            Failure::invalid("dataset_not_found", format!("`{dataset}` is not a file"))
                .remedy("check the path"),
        );
    }

    // The engine's flag is `--file`; the `ds` flag is `--dataset` because
    // "file" says nothing about what the file is, and a caller reading
    // `--dataset <path>` in help knows immediately what to pass. Translating
    // here is the adapter's job — the two vocabularies are allowed to differ,
    // and `solar_engine_flags_are_real` proves this one is not a guess.
    let args: Vec<OsString> = vec![OsString::from("--file"), OsString::from(dataset)];
    let completed = DS_SOLAR.call("verify-weather", &args, DISCOVERY_TIMEOUT)?;
    if !completed.succeeded() {
        return Err(DS_SOLAR.failure_from(&completed, "verify-weather"));
    }

    Ok(json!({
        "dataset": dataset,
        "verified": true,
        "engine": completed.stdout.trim(),
    }))
}

pub fn render(data: &Value) -> String {
    format!(
        "verified  {}\n{}",
        data["dataset"].as_str().unwrap_or(""),
        data["engine"].as_str().unwrap_or(""),
    )
}
