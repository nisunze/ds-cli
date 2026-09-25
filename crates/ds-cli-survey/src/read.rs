//! One survey form's entries, with every field value and the survey media
//! each entry references: the rows the map loads, read headlessly.
//!
//! `entries select` deliberately returns geometry and identities only. A
//! survey report needs the values and the photos, and the operator's rule is
//! that what the UI can do under the user's JWT, the CLI does too (feedback
//! ee3f371f, 2026-09-25).

use std::io::Write;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::survey_entries_read::SURVEY_ENTRIES_READ_MAX_LIMIT;
use ds_client_core::{SurveyEntriesRead, SurveyEntriesReadRequest, SurveyEntry};
use serde_json::{Value, json};

const FORM: Arg = Arg::value(
    "form",
    "<form-slug>",
    "Exact project form slug from `ds survey project-forms list`.",
)
.required();
const UPDATED_AFTER: Arg = Arg::value(
    "updated-after",
    "<rfc3339>",
    "Only entries changed after this instant.",
);
const BBOX: Arg = Arg::value(
    "bbox",
    "<west,south,east,north>",
    "Only entries inside this WGS84 box.",
);
const LIMIT: Arg = Arg::value(
    "limit",
    "<1-5000>",
    "Most entries returned; the total is always reported.",
)
.default("100");
const INCLUDE_DELETED: Arg = Arg::switch("include-deleted", "Also return deleted entries.");
const OUT: Arg = Arg::value(
    "out",
    "<file.geojson>",
    "Write the entries as a new GeoJSON file; never overwritten.",
);
const LANE: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Deployment lane; stable is the default.",
)
.default("stable")
.choices(&["stable", "canary"]);

const REFUSALS: &[Refusal] = &[
    Refusal {
        code: "survey_entries_invalid",
        when: "a flag violates the local grammar",
        remedy: "pass one exact form slug, an RFC 3339 instant, four ordered WGS84 coordinates, and a limit from 1 through 5000",
    },
    Refusal {
        code: "survey_entries_scope_not_found",
        when: "the project or form is not visible to the verified user",
        remedy: "verify --project and pass one exact slug from `ds survey project-forms list`",
    },
    Refusal {
        code: "survey_entries_auth_rejected",
        when: "the read route rejects the verified identity or its project authority",
        remedy: "verify account and form authority in the project",
    },
    Refusal {
        code: "survey_entries_transient",
        when: "the survey read service is unavailable, or failed part-way through the form",
        remedy: "retry without changing the request",
    },
    Refusal {
        code: "survey_entries_unreadable",
        when: "the stream was cut off, lost rows against its summary, or broke its line grammar",
        remedy: "retry once, then report it with `ds feedback submit`",
    },
    Refusal {
        code: "survey_entries_output",
        when: "--out names an existing or unwritable file",
        remedy: "choose a new writable path",
    },
    ds_cli_auth::SIGNED_OUT_REFUSAL,
    Refusal {
        code: "project_required",
        when: "--project is absent, blank or untrimmed",
        remedy: "pass one exact ds_project value from ds auth project list",
    },
    Refusal {
        code: "auth_transient",
        when: "native identity restoration is temporarily unavailable",
        remedy: "retry without changing local state",
    },
    Refusal {
        code: "native_profile_not_configured",
        when: "the exact packaged native profile is unavailable",
        remedy: "install one complete ds release",
    },
];

pub static COMMAND: Command = Command {
    id: "survey.entries.read",
    path: &["survey", "entries", "read"],
    contract: 1,
    chapter: Chapter::Survey,
    summary: "Read a survey form's entries: field values and photo references.",
    purpose: "Use to report on surveyed assets: every field value the map shows for one project form, and each entry's survey photos (object path and thumbnail) ready for `ds survey photo fetch`. Filters are the map's own: changed after an instant, inside a box, deleted or not. The total is always reported; raise --limit or write --out for the whole form.",
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT,
        FORM,
        UPDATED_AFTER,
        BBOX,
        LIMIT,
        INCLUDE_DELETED,
        OUT,
        LANE,
    ],
    output: "The project, form and filters; the total, how many were returned and whether that is all; each entry's geometry, properties and photos (property, object path, thumbnail path). With --out, the entries go to a GeoJSON file and the output names it.",
    examples: &[
        Example {
            command: "ds survey entries read --project <exact-id> --form <form-slug> --output json",
            note: "Up to 100 entries with every value and photo reference, plus the form's total. Resolve the exact slug with `ds survey project-forms list`.",
            runnable: false,
        },
        Example {
            command: "ds survey entries read --project <exact-id> --form <form-slug> --limit 5000 --out entries.geojson",
            note: "The whole form to a GeoJSON file; each feature lists its photos under `media`.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/survey.md"),
    search: &[
        "survey photos",
        "survey values",
        "survey report",
        "entries attributes",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    // Every caller-controlled byte is checked before any credential is read.
    let request = parse(inputs)?;
    let out = inputs.value("out");
    let headless = ds_cli_auth::survey_entries_read(
        inputs.require("lane")?,
        inputs.require("project")?,
        &request,
    )?;
    let read = headless.result();
    let mut data = json!({
        "lane": headless.lane(),
        "project": {"ds_project": headless.project_id()},
        "form": read.form,
        "filters": {
            "updated_after": request.updated_after(),
            "bbox": inputs.value("bbox"),
            "include_deleted": inputs.switch("include-deleted"),
        },
        "total": read.total,
        "returned": read.entries.len(),
        "truncated": read.truncated,
        "media_count": read.entries.iter().map(|entry| entry.media.len()).sum::<usize>(),
        "sync": read.sync,
    });
    match out {
        Some(path) => {
            write_new(path, read)?;
            data["file"] = json!(path);
        }
        None => data["entries"] = read.entries.iter().map(entry_json).collect(),
    }
    Ok(data)
}

fn entry_json(entry: &SurveyEntry) -> Value {
    json!({
        "geometry": entry.feature.get("geometry").cloned().unwrap_or(Value::Null),
        "properties": entry.feature.get("properties").cloned().unwrap_or(Value::Null),
        "media": media_json(entry),
    })
}

fn media_json(entry: &SurveyEntry) -> Value {
    entry
        .media
        .iter()
        .map(|media| {
            json!({
                "property": media.property,
                "reference": media.reference,
                "bucket": media.bucket,
                "object_path": media.object_path,
                "thumbnail_object_path": media.thumbnail_object_path,
            })
        })
        .collect()
}

/// A FeatureCollection whose features are the streamed rows unchanged, each
/// with its photos as a `media` foreign member.
fn write_new(path: &str, read: &SurveyEntriesRead) -> Result<(), Failure> {
    let features: Vec<Value> = read
        .entries
        .iter()
        .map(|entry| {
            let mut feature = entry.feature.clone();
            feature["media"] = media_json(entry);
            feature
        })
        .collect();
    let document = json!({
        "type": "FeatureCollection",
        "ds_project": read.project,
        "form": read.form,
        "total": read.total,
        "truncated": read.truncated,
        "features": features,
    });
    let output = |error: std::io::Error| {
        Failure::invalid("survey_entries_output", error.to_string())
            .remedy("choose a new writable path")
    };
    let encoded = serde_json::to_vec(&document).expect("closed GeoJSON document");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(output)?;
    if let Err(error) = file.write_all(&encoded).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(output(error));
    }
    Ok(())
}

fn parse(inputs: &Inputs) -> Result<SurveyEntriesReadRequest, Failure> {
    let bbox = inputs.value("bbox").map(parse_bbox).transpose()?;
    let limit = inputs
        .require("limit")?
        .parse::<usize>()
        .ok()
        .filter(|limit| (1..=SURVEY_ENTRIES_READ_MAX_LIMIT).contains(limit))
        .ok_or_else(|| invalid("`--limit` must be an integer from 1 through 5000"))?;
    SurveyEntriesReadRequest::new(
        inputs.require("form")?,
        inputs.value("updated-after"),
        bbox,
        inputs.switch("include-deleted"),
        Some(limit),
    )
    .map_err(|error| invalid(error.to_string()))
}

fn parse_bbox(raw: &str) -> Result<[f64; 4], Failure> {
    let values = raw
        .split(',')
        .map(|value| value.trim().parse::<f64>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| invalid("`--bbox` must contain exactly four decimal coordinates"))?;
    values
        .try_into()
        .map_err(|_| invalid("`--bbox` must contain west,south,east,north"))
}

fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid("survey_entries_invalid", message)
        .remedy("read `ds survey entries read --help` and pass only its typed flags")
}

pub fn render(data: &Value) -> String {
    let project = data["project"]["ds_project"].as_str().unwrap_or("project");
    let form = data["form"].as_str().unwrap_or("form");
    let returned = data["returned"].as_u64().unwrap_or(0);
    let total = data["total"].as_u64().unwrap_or(0);
    let media = data["media_count"].as_u64().unwrap_or(0);
    let mut text = format!("{project}/{form}  {returned} of {total} entries  {media} photos\n");
    if data["truncated"].as_bool().unwrap_or(false) {
        text.push_str(
            "  NOT ALL — raise --limit (up to 5000) or narrow with --bbox/--updated-after\n",
        );
    }
    if let Some(file) = data["file"].as_str() {
        text.push_str(&format!("  written to {file}\n"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::args::parse as parse_args;

    fn inputs(arguments: &[&str]) -> Inputs {
        parse_args(
            &COMMAND,
            &arguments
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>(),
        )
        .expect("closed arguments")
    }

    #[test]
    fn filters_parse_before_auth_and_the_default_bound_is_a_hundred() {
        let request = parse(&inputs(&[
            "--project",
            "p1",
            "--form",
            "transformer_survey",
            "--bbox",
            "29.7, -2.1,29.8,-2.0",
            "--updated-after",
            "2026-09-01T00:00:00Z",
        ]))
        .unwrap();
        assert_eq!(request.limit(), 100);
        assert_eq!(request.form(), "transformer_survey");
        assert_eq!(request.updated_after(), Some("2026-09-01T00:00:00Z"));
        for bad in [
            &["--limit", "0"][..],
            &["--limit", "5001"],
            &["--bbox", "1,2,3"],
            &["--bbox", "30,-2,29,-1"],
            &["--updated-after", "yesterday"],
        ] {
            let mut arguments = vec!["--project", "p1", "--form", "f"];
            arguments.extend_from_slice(bad);
            let failure = parse(&inputs(&arguments)).unwrap_err();
            assert_eq!(failure.code(), "survey_entries_invalid", "{bad:?}");
        }
    }

    #[test]
    fn a_partial_read_says_so_in_words() {
        let text = render(
            &json!({"project":{"ds_project":"p1"},"form":"f","returned":100,
            "total":247,"media_count":88,"truncated":true}),
        );
        assert!(text.contains("100 of 247 entries  88 photos"));
        assert!(text.contains("NOT ALL"));
    }
}
