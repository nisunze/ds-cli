//! One survey form's entries, with every field value and the survey media
//! each entry references, answered from the core's copy of the form.
//!
//! `entries select` deliberately returns geometry and identities only. A
//! survey report needs the values and the photos, and the operator's rule is
//! that what the UI can do under the user's JWT, the CLI does too (feedback
//! ee3f371f, 2026-09-25). The core holds one copy per account, project and
//! form (docs/contracts/survey-hold.md in ds-command-kernel): every decision
//! — reuse, refresh the changes, read again, which filter — is the kernel's
//! `survey::hold`, and the bytes are `ds_project_data::survey_hold`'s. This
//! command parses flags and renders.

use std::io::Write;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::survey_entries_read::{SURVEY_ENTRIES_READ_MAX_LIMIT, entry_media};
use ds_client_core::{SurveyEntriesRead, SurveyEntriesReadRequest, SurveyEntry};
use ds_command_kernel::project_dataset_cache::Scope;
use ds_command_kernel::survey::hold::{self, AdminBoundary, Filter, Manifest, Refresh, View};
use ds_command_kernel::time::OffsetDateTime;
use ds_project_data::survey_hold::{self as store, HoldError};
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
const ADMIN_BOUNDARY: Arg = Arg::value(
    "admin-boundary",
    "<code>",
    "Only entries in this administrative boundary.",
);
const BOUNDARY: Arg = Arg::value(
    "boundary",
    "<polygon.geojson>",
    "Only entries inside this drawn GeoJSON polygon.",
);
const DATE_FROM: Arg = Arg::value("date-from", "<YYYY-MM-DD>", "Only entries from this day.");
const DATE_TO: Arg = Arg::value("date-to", "<YYYY-MM-DD>", "Only entries until this day.");
const SURVEYOR: Arg = Arg::repeated("surveyor", "<name>", "Only this surveyor's entries.");
const REFRESH: Arg = Arg::value(
    "refresh",
    "<auto|delta|full|local>",
    "Reuse, refresh or re-read the held copy; local never goes online.",
)
.default("auto")
.choices(&["auto", "delta", "full", "local"]);
const LIMIT: Arg = Arg::value(
    "limit",
    "<1-5000>",
    "Most entries returned; the total is always reported.",
)
.default("100");
const INCLUDE_DELETED: Arg = Arg::switch(
    "include-deleted",
    "Also return deleted entries (read from the cloud; never held).",
);
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
        remedy: "read `ds survey entries read --help` and pass only its typed flags",
    },
    Refusal {
        code: "survey_hold_not_held",
        when: "--refresh local and this machine holds no copy of the form under that filter",
        remedy: "drop --refresh local to read it once; `ds survey local status` lists what is held",
    },
    Refusal {
        code: "survey_hold_store",
        when: "the core's survey copy could not be read or written",
        remedy: "check disk space and the layer root, then retry",
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
        when: "the stream was cut off, lost rows against its summary, or broke its line grammar; the held copy is unchanged",
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
    contract: 2,
    chapter: Chapter::Survey,
    summary: "Read a survey form's entries: field values and photo references.",
    purpose: "Use to report on surveyed assets: every field value the map shows for one project form, and each entry's photos (object and thumbnail path) for `ds survey photo fetch`. Answered from this machine's copy of the form: reused when fresh, else only the changes are fetched; `source` says which and `held` says under which filter and when. Filters are the map's working area. The total is always reported.",
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT,
        FORM,
        UPDATED_AFTER,
        BBOX,
        ADMIN_BOUNDARY,
        BOUNDARY,
        DATE_FROM,
        DATE_TO,
        SURVEYOR,
        REFRESH,
        LIMIT,
        INCLUDE_DELETED,
        OUT,
        LANE,
    ],
    output: "The project, form and filter; source (held, delta, replace or cloud) and the held copy (filter, rows, refreshed, cursor); the total, how many were returned and whether that is all; each entry's geometry, properties and photos. With --out, the entries go to a GeoJSON file.",
    examples: &[
        Example {
            command: "ds survey entries read --project <exact-id> --form <form-slug> --output json",
            note: "Up to 100 entries with every value and photo reference, plus the form's total. Resolve the exact slug with `ds survey project-forms list`.",
            runnable: false,
        },
        Example {
            command: "ds survey entries read --project <exact-id> --form <form-slug> --refresh local --limit 5000 --out entries.geojson",
            note: "The whole held form to a GeoJSON file without going online; each feature lists its photos under `media`.",
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
        "held survey",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    // Every caller-controlled byte is checked before any credential is read.
    let request = parse(inputs)?;
    if inputs.switch("include-deleted") {
        return cloud(inputs, &request);
    }
    let filter = parse_filter(inputs)?;
    let refresh = match inputs.require("refresh")? {
        "delta" => Refresh::Delta,
        "full" => Refresh::Full,
        "local" => Refresh::Local,
        _ => Refresh::Auto {
            max_age_seconds: hold::DEFAULT_MAX_AGE_SECONDS,
        },
    };
    let view = View {
        bbox: filter.bbox,
        updated_after: request.updated_after().map(str::to_owned),
    };
    let root = ds_layer_store::default_root().map_err(store_failure)?;
    let now = OffsetDateTime::now_utc();
    let lane = inputs.require("lane")?;
    let project = inputs.require("project")?;
    let form = request.form();
    let answer = |uid: &str, fetch: &mut dyn FnMut(&Value) -> Result<Vec<u8>, Failure>| {
        let scope = Scope {
            principal: uid.to_owned(),
            project: project.to_owned(),
        };
        let held = store::refresh(&root, &scope, form, &filter, refresh, now, |body| {
            fetch(body)
        })
        .map_err(hold_failure)?;
        let rows = store::view(&root, &scope, &held.manifest, &view).map_err(store_failure)?;
        Ok::<_, Failure>((held.source, held.manifest, rows))
    };
    // Offline first: the account is observed from this machine's providers
    // and the kernel decides over what is held. A fresh copy (or `local`)
    // answers without restoring a credential; only when the kernel asks for
    // the cloud does the read restore the lane's JWT and fetch.
    let observed = match refresh {
        Refresh::Local | Refresh::Auto { .. } => {
            ds_cli_auth::probe_headless_identity_for_named_project(lane)?
        }
        _ => None,
    };
    let offline_answer = match &observed {
        Some(identity) => {
            let mut offline = |_: &Value| -> Result<Vec<u8>, Failure> {
                Err(Failure::failed(
                    "survey_hold_needs_cloud",
                    "the held copy needs a refresh",
                ))
            };
            match answer(identity.uid(), &mut offline) {
                Ok(held) => Some((identity.lane().to_owned(), project.to_owned(), held)),
                Err(failure) if failure.code() == "survey_hold_needs_cloud" => None,
                Err(failure) => return Err(failure),
            }
        }
        None if matches!(refresh, Refresh::Local) => {
            return Err(Failure::unauthorized(
                "headless_signed_out",
                "this machine has no signed-in account on this lane",
            )
            .remedy("run `ds account connect`"));
        }
        None => None,
    };
    let (lane_token, project_id, (source, manifest, rows)) = match offline_answer {
        Some(held) => held,
        None => {
            let headless = ds_cli_auth::survey_hold(lane, project, answer)?;
            (
                headless.lane().to_owned(),
                headless.project_id().to_owned(),
                headless.into_result(),
            )
        }
    };
    let limit = request.limit();
    let read = SurveyEntriesRead {
        project: project_id.clone(),
        form: form.to_owned(),
        total: rows.len() as u64,
        truncated: rows.len() > limit,
        entries: rows
            .into_iter()
            .take(limit)
            .map(|feature| SurveyEntry {
                media: entry_media(&feature),
                feature,
            })
            .collect(),
        sync: None,
    };
    let mut data = answer_json(&lane_token, &read, inputs, &request);
    data["source"] = json!(source.as_str());
    data["held"] = held_json(&manifest);
    finish(data, &read, inputs.value("out"))
}

/// Deleted entries are never held, so a read that wants them asks the cloud
/// directly, as before the hold, and says so.
fn cloud(inputs: &Inputs, request: &SurveyEntriesReadRequest) -> Result<Value, Failure> {
    let headless = ds_cli_auth::survey_entries_read(
        inputs.require("lane")?,
        inputs.require("project")?,
        request,
    )?;
    let read = headless.result();
    let mut data = answer_json(headless.lane(), read, inputs, request);
    data["source"] = json!("cloud");
    data["sync"] = json!(read.sync);
    finish(data, read, inputs.value("out"))
}

fn answer_json(
    lane: &str,
    read: &SurveyEntriesRead,
    inputs: &Inputs,
    request: &SurveyEntriesReadRequest,
) -> Value {
    json!({
        "lane": lane,
        "project": {"ds_project": read.project},
        "form": read.form,
        "filters": {
            "updated_after": request.updated_after(),
            "bbox": inputs.value("bbox"),
            "admin_boundary": inputs.value("admin-boundary"),
            "boundary": inputs.value("boundary"),
            "date_from": inputs.value("date-from"),
            "date_to": inputs.value("date-to"),
            "surveyors": inputs.repeated("surveyor"),
            "include_deleted": inputs.switch("include-deleted"),
        },
        "total": read.total,
        "returned": read.entries.len(),
        "truncated": read.truncated,
        "media_count": read.entries.iter().map(|entry| entry.media.len()).sum::<usize>(),
    })
}

fn held_json(manifest: &Manifest) -> Value {
    json!({
        "filter": manifest.filter,
        "whole_form": manifest.filter.is_whole_form(),
        "filter_sha256": manifest.filter_sha256,
        "rows": manifest.rows,
        "created_at": manifest.created_at,
        "refreshed_at": manifest.refreshed_at,
        "cursor": manifest.cursor,
        "last": manifest.last,
    })
}

fn finish(mut data: Value, read: &SurveyEntriesRead, out: Option<&str>) -> Result<Value, Failure> {
    match out {
        Some(path) => {
            write_new(path, read)?;
            data["file"] = json!(path);
        }
        None => data["entries"] = read.entries.iter().map(entry_json).collect(),
    }
    Ok(data)
}

fn hold_failure(error: HoldError<Failure>) -> Failure {
    match error {
        HoldError::Fetch(failure) => failure,
        HoldError::NotHeld => Failure::invalid(
            "survey_hold_not_held",
            "this machine holds no copy of the form under that filter",
        )
        .remedy(
            "drop --refresh local to read it once; `ds survey local status` lists what is held",
        ),
        HoldError::Refused(message) => Failure::unavailable("survey_entries_unreadable", message)
            .remedy("retry once, then report it with `ds feedback submit`"),
        HoldError::Store(message) => store_failure(message),
    }
}

fn store_failure(message: String) -> Failure {
    Failure::unavailable("survey_hold_store", message)
        .remedy("check disk space and the layer root, then retry")
}

/// The working-area filter, canonical, checked before any credential is read.
fn parse_filter(inputs: &Inputs) -> Result<Filter, Failure> {
    let boundary = inputs
        .value("boundary")
        .map(|path| {
            let bytes = std::fs::read(path)
                .map_err(|error| invalid(format!("`--boundary` {path}: {error}")))?;
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|_| invalid("`--boundary` is not GeoJSON"))?;
            // A Feature or a bare geometry; the filter holds the geometry.
            Ok::<_, Failure>(match value.get("geometry") {
                Some(geometry) if value["type"] == "Feature" => geometry.clone(),
                _ => value,
            })
        })
        .transpose()?;
    Filter {
        bbox: inputs.value("bbox").map(parse_bbox).transpose()?,
        admin_boundary: inputs.value("admin-boundary").map(|code| AdminBoundary {
            code: code.to_owned(),
            name: String::new(),
        }),
        boundary,
        date_from: inputs.value("date-from").map(str::to_owned),
        date_to: inputs.value("date-to").map(str::to_owned),
        surveyors: inputs.repeated("surveyor").to_vec(),
    }
    .canonical()
    .map_err(invalid)
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
