//! `ds survey export` — one governed survey export of the active project.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;

const FORMAT_ARG: Arg = Arg::value("format", "<format>", "Output format.")
    .required()
    .choices(&["gpkg", "shp", "kmz", "geojson", "xlsx", "csv"]);
const FORM_ARG: Arg = Arg::repeated(
    "form",
    "<slug>",
    "Restrict to these forms. Repeat; omit for every form.",
);
const FROM_ARG: Arg = Arg::value(
    "from",
    "<yyyy-mm-dd>",
    "Entries collected on or after this date.",
);
const TO_ARG: Arg = Arg::value(
    "to",
    "<yyyy-mm-dd>",
    "Entries collected on or before this date.",
);
const SURVEYOR_ARG: Arg = Arg::repeated(
    "surveyor",
    "<email>",
    "Restrict to entries created by these surveyors. Repeat.",
);
const BBOX_ARG: Arg = Arg::value(
    "bbox",
    "<w,s,e,n>",
    "Restrict to entries inside this WGS84 box.",
);

pub static COMMAND: Command = Command {
    id: "survey.export",
    path: &["survey", "export"],
    contract: 1,
    summary: "Export the active project's survey data to a file format.",
    purpose: "\
Runs the same governed export the application's Export dialog runs, under the \
signed-in session, over the active project's synced survey data — every form, \
or the forms, dates, surveyors and box given. The artifact is written to the \
project's storage and a short-lived download link is returned; the export is \
a durable artifact of record, so dispatch requires --yes.",
    effect: Effect::ArtifactWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        FORMAT_ARG,
        FORM_ARG,
        FROM_ARG,
        TO_ARG,
        SURVEYOR_ARG,
        BBOX_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "\
`project`, `format`, `filename`, `total_features`, `file_size_bytes`, \
`blob_path` (the artifact of record), `download_url` (signed, expiring) and \
the `filters` that were applied.",
    examples: &[Example {
        command: "ds survey export --format gpkg --form edcl_customers_survey --from 2026-08-01 --yes --output json",
        note: "The link expires; keep blob_path as the durable reference.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::SURVEY_REFUSED,
        crate::INVALID_FORM,
        crate::INVALID_INPUT,
        crate::CONFIRMATION_REQUIRED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
    ],
    reference: Some("docs/reference/survey.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    arguments.insert("format".into(), json!(inputs.require("format")?));
    let forms = inputs.repeated("form");
    if forms.len() > crate::MAX_EXPORT_SELECTORS {
        return Err(Failure::invalid(
            "invalid_input",
            format!(
                "at most {} --form values are accepted",
                crate::MAX_EXPORT_SELECTORS
            ),
        )
        .remedy(crate::INVALID_INPUT.remedy));
    }
    if !forms.is_empty() {
        let slugs = forms
            .iter()
            .map(|f| crate::form_slug(f).map(str::to_string))
            .collect::<Result<Vec<_>, _>>()?;
        arguments.insert("forms".into(), json!(slugs));
    }
    if let Some(from) = inputs.value("from") {
        arguments.insert("dateFrom".into(), json!(crate::date(from, "from")?));
    }
    if let Some(to) = inputs.value("to") {
        arguments.insert("dateTo".into(), json!(crate::date(to, "to")?));
    }
    let surveyors = inputs.repeated("surveyor");
    if surveyors.len() > crate::MAX_EXPORT_SELECTORS {
        return Err(Failure::invalid(
            "invalid_input",
            format!(
                "at most {} --surveyor values are accepted",
                crate::MAX_EXPORT_SELECTORS
            ),
        )
        .remedy(crate::INVALID_INPUT.remedy));
    }
    if !surveyors.is_empty() {
        for surveyor in surveyors {
            if surveyor.len() > 160 || !surveyor.contains('@') {
                return Err(
                    Failure::invalid("invalid_input", "each --surveyor must be an email")
                        .remedy(crate::INVALID_INPUT.remedy),
                );
            }
        }
        arguments.insert("surveyors".into(), json!(surveyors));
    }
    if let Some(bbox) = inputs.value("bbox") {
        arguments.insert("bbox".into(), json!(crate::bbox(bbox)?));
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::EXPORT,
        Value::Object(arguments),
        crate::EXPORT_TIMEOUT,
    )
    .map(receipt)
    .map_err(crate::classify_survey_failure)
}

fn receipt(result: Value) -> Value {
    json!({
        "project": result["project"],
        "format": result["format"],
        "filename": result["filename"],
        "total_features": result["totalFeatures"],
        "file_size_bytes": result["fileSizeBytes"],
        "blob_path": result["blobPath"],
        "download_url": result["downloadUrl"],
        "download_url_expires": true,
        "filters": result["filters"],
    })
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "exported {}  ·  {}  ·  {}\n",
        data["project"].as_str().unwrap_or("?"),
        data["format"].as_str().unwrap_or("?"),
        crate::plural(data["total_features"].as_u64().unwrap_or(0), "feature"),
    );
    if let Some(name) = data["filename"].as_str() {
        out.push_str(&format!(
            "  {}  ({} bytes)\n",
            name,
            data["file_size_bytes"].as_u64().unwrap_or(0)
        ));
    }
    if let Some(url) = data["download_url"].as_str() {
        out.push_str(&format!("  download (expires): {url}\n"));
    }
    if let Some(path) = data["blob_path"].as_str() {
        out.push_str(&format!("  artifact: {path}\n"));
    }
    out
}
