//! `ds survey moments` — the survey photos this machine holds for a project.
//!
//! The Server's survey-media store (rows in the lane's `store.sqlite`,
//! bytes under `survey-media/`) is the authority for what this machine
//! holds; this module reads it and hands the rows to the kernel, which
//! decides everything shown (`ds_command_kernel::survey_moments`). Read-only
//! over the store, like `ds report outbox status`: no session, no gateway,
//! no running Server. The same command answered by the browser's WASM
//! kernel over its own photo cache is the same list.
//!
//! Rotation is NOT a third command: `ds survey photo rotate` is the one
//! rotation (the properties panel's semantics), and without `--out` it
//! holds its result here, where `list` shows it as `waiting` until
//! `ds survey photo publish --path` settles it.

use std::path::{Path, PathBuf};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::survey_moments::{self, Filter, MediaRecord, SyncState};
use ds_command_kernel::sync_store::Fence;
use serde_json::{Value, json};

pub const PROJECT_ARG: Arg = Arg::value(
    "project",
    "<project-id>",
    "Exact project whose held survey photos are read.",
)
.required();
pub const SERVER_STATE_DIR_ARG: Arg = Arg::value(
    "server-state-dir",
    "<absolute-path>",
    "Matching `ds server serve --state-dir` when the Server uses a custom state root.",
);
const FORM_ARG: Arg = Arg::repeated(
    "form",
    "<form-slug>",
    "Only photos of this form; repeat for any of several.",
);
const SINCE_ARG: Arg = Arg::value(
    "since",
    "<YYYY-MM-DD|RFC3339>",
    "Inclusive lower bound on the moment's clock (capture time, else cache time).",
);
const UNTIL_ARG: Arg = Arg::value(
    "until",
    "<YYYY-MM-DD|RFC3339>",
    "Inclusive upper bound; a bare day keeps the whole UTC day.",
);
const SYNC_ARG: Arg = Arg::value(
    "sync",
    "<waiting|synced>",
    "Only photos waiting to publish, or only those equal to the published head.",
)
.choices(&["waiting", "synced"]);
const TEXT_ARG: Arg = Arg::value("text", "<substring>", "Case-insensitive file-name search.");
const LIMIT_ARG: Arg = Arg::value(
    "limit",
    "<1-500>",
    "Rows answered; `more` says if the filter kept more.",
)
.default("60");
const PATH_ARG: Arg = Arg::value(
    "path",
    "<object-path>",
    "Canonical original survey object path, as list answers it.",
)
.required();

pub const ROOT_INVALID: Refusal = Refusal {
    code: "local_root_invalid",
    when: "the Server state root cannot be resolved from the lane and --server-state-dir",
    remedy: "omit --server-state-dir, or pass the same absolute path `ds server serve` uses",
};
pub const STORE_UNREADABLE: Refusal = Refusal {
    code: "local_store_unreadable",
    when: "the lane's sync store or survey-media directory cannot be read on this machine",
    remedy: "check the Server state directory's permissions, or pass the same --server-state-dir as ds server serve",
};
const INVALID_FILTER: Refusal = Refusal {
    code: "invalid_filter",
    when: "a bound is not a YYYY-MM-DD day or RFC 3339 instant, since is after until, the form is not a slug, text exceeds 128 characters or limit is outside 1 through 500",
    remedy: "correct the named criterion; every other criterion is unchanged",
};
const NOT_HELD: Refusal = Refusal {
    code: "moment_not_held",
    when: "this machine holds no survey photo at that path for the project",
    remedy: "read the paths from `ds survey moments list` first",
};
const NOT_SURVEY_MEDIA: Refusal = Refusal {
    code: "not_survey_media",
    when: "the path is not a survey original of the project (a thumbnail, a URL, another project)",
    remedy: "pass the canonical original object path the list answered",
};

pub static LIST_COMMAND: Command = Command {
    id: "survey.moments.list",
    path: &["survey", "moments", "list"],
    contract: 1,
    summary: "List the survey photos this machine holds for a project.",
    purpose: "Reads the Server's survey-media store — every survey original held on this machine for the project, with its form, entry, clock, sync state and thumbnail — and answers the kernel's gallery: filtered, newest first, N of M. `waiting` rows are rotations not yet published; `synced` rows equal the published head. Needs no credential and no running Server.",
    chapter: Chapter::Survey,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        PROJECT_ARG,
        FORM_ARG,
        SINCE_ARG,
        UNTIL_ARG,
        SYNC_ARG,
        TEXT_ARG,
        LIMIT_ARG,
        SERVER_STATE_DIR_ARG,
        crate::LANE,
    ],
    output: "`moments[]` (path, file, form, entry, thumbnail_path, media_type, size, state, cached_at_ms, captured_at_ms, when_ms, location, navigate), `count`, `matched`, `total`, `more`, `waiting`, `synced`, `forms[]`, the applied `filter`.",
    examples: &[
        Example {
            command: "ds survey moments list --project <project-id> --lane canary --output json",
            note: "Everything held for the project, newest first.",
            runnable: false,
        },
        Example {
            command: "ds survey moments list --project <project-id> --form <form-slug> --since 2026-09-01 --sync waiting --output json",
            note: "One form's rotations since 1 September still waiting; slugs are `.data.forms[]` of the unfiltered list or `ds survey forms list`.",
            runnable: false,
        },
    ],
    refusals: &[INVALID_FILTER, ROOT_INVALID, STORE_UNREADABLE],
    reference: Some("docs/reference/survey.md"),
    search: &[
        "gallery",
        "photo cache",
        "pictures",
        "images",
        "offline photos",
        "held photos",
    ],
    requires: Requires::Server,
    availability: || ds_cli_contract::spec::Availability::Available,
};

pub static READ_COMMAND: Command = Command {
    id: "survey.moments.read",
    path: &["survey", "moments", "read"],
    contract: 1,
    summary: "Describe one held survey photo and where its bytes are.",
    purpose: "The viewer's details for one moment the machine holds: form, entry, file, clock, size, sync state, the pinned generations and, on the Server, the bundle directory holding original.bin, thumbnail.jpeg and manifest.json. Read-only.",
    chapter: Chapter::Survey,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[PROJECT_ARG, PATH_ARG, SERVER_STATE_DIR_ARG, crate::LANE],
    output: "`moment` (as list), `bundle` directory, `original` and `thumbnail` file paths, `degrees`, `source_generation`, `published_generation`.",
    examples: &[Example {
        command: "ds survey moments read --project <project-id> --path <object-path> --lane canary --output json",
        note: "Open `.data.original` to see the held bytes.",
        runnable: false,
    }],
    refusals: &[NOT_HELD, NOT_SURVEY_MEDIA, ROOT_INVALID, STORE_UNREADABLE],
    reference: Some("docs/reference/survey.md"),
    search: &["photo info", "photo file", "picture details"],
    requires: Requires::Server,
    availability: || ds_cli_contract::spec::Availability::Available,
};

pub fn server_state(inputs: &Inputs) -> Result<PathBuf, Failure> {
    ds_compute_runtime::server_state_directory(
        inputs.require("lane")?,
        inputs.value("server-state-dir").map(Path::new),
    )
    .map_err(|error| Failure::invalid(ROOT_INVALID.code, error).remedy(ROOT_INVALID.remedy))
}

pub fn unreadable(error: impl std::fmt::Display) -> Failure {
    Failure::failed(STORE_UNREADABLE.code, error.to_string()).remedy(STORE_UNREADABLE.remedy)
}

/// The store's fence on this machine, read from protected local state — how
/// the Server derives its own, so a rotation this CLI holds is one the
/// Server's pump could publish.
pub fn fence(lane: &str) -> Result<Fence, Failure> {
    let principal = ds_cli_auth::headless_principal(lane)?;
    Ok(Fence {
        account: principal.account_uid().to_owned(),
        deployment: principal.deployment().to_owned(),
        install_id: principal.install_id().to_owned(),
    })
}

/// Where one object's bundle lives below the state root.
pub fn media_dir(state: &Path, project: &str, path: &str) -> PathBuf {
    state
        .join(survey_moments::MEDIA_ROOT)
        .join(project)
        .join(survey_moments::media_key(path))
}

/// The project's records, read without a session; a machine with no store
/// holds none.
pub fn records(state: &Path, project: &str) -> Result<Vec<MediaRecord>, Failure> {
    match ds_sync_store::Store::open_read_only(&state.join("store.sqlite")).map_err(unreadable)? {
        Some(store) => store.survey_media_of_project(project).map_err(unreadable),
        None => Ok(Vec::new()),
    }
}

pub fn record(state: &Path, project: &str, path: &str) -> Result<Option<MediaRecord>, Failure> {
    match ds_sync_store::Store::open_read_only(&state.join("store.sqlite")).map_err(unreadable)? {
        Some(store) => store.survey_media_record(project, path).map_err(unreadable),
        None => Ok(None),
    }
}

/// The kernel's refusal, re-raised under the code it named and the class
/// this surface declares for it.
fn refused(refusal: survey_moments::Refusal) -> Failure {
    let message = refusal.message;
    let failure = match refusal.code {
        "moment_not_held" => Failure::conflict("moment_not_held", message),
        "not_survey_media" => Failure::invalid("not_survey_media", message),
        _ => Failure::invalid("invalid_filter", message),
    };
    failure.remedy(refusal.remedy)
}

pub fn list(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = crate::text(inputs.require("project")?, "project", 200)?;
    let state = server_state(inputs)?;
    let rows: Vec<_> = records(&state, project)?
        .iter()
        .map(MediaRecord::moment_row)
        .collect();
    let filter = Filter {
        form: None,
        forms: inputs.repeated("form").to_vec(),
        since: inputs.value("since").map(str::to_string),
        until: inputs.value("until").map(str::to_string),
        sync: match inputs.value("sync") {
            Some("waiting") => Some(SyncState::Waiting),
            Some("synced") => Some(SyncState::Synced),
            _ => None,
        },
        text: inputs.value("text").map(str::to_string),
        limit: match inputs.value("limit") {
            Some(raw) => Some(raw.parse().map_err(|_| {
                Failure::invalid(
                    INVALID_FILTER.code,
                    format!("limit `{raw}` is not a number"),
                )
                .remedy(INVALID_FILTER.remedy)
            })?),
            None => None,
        },
    };
    let answer = survey_moments::list(project, &rows, &filter).map_err(refused)?;
    let mut value = json!(answer);
    value["root"] = json!(state.join(survey_moments::MEDIA_ROOT).join(project));
    Ok(value)
}

pub fn read(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = crate::text(inputs.require("project")?, "project", 200)?;
    let path = inputs.require("path")?;
    let state = server_state(inputs)?;
    let held = record(&state, project, path)?;
    let row = held.as_ref().map(MediaRecord::moment_row);
    let moment = survey_moments::read(project, path, row.as_ref()).map_err(refused)?;
    let record = held.expect("a described moment is a held record");
    let bundle = media_dir(&state, project, path);
    Ok(json!({
        "moment": moment,
        "bundle": bundle,
        "original": bundle.join("original.bin"),
        "thumbnail": bundle.join("thumbnail.jpeg"),
        "manifest": bundle.join("manifest.json"),
        "degrees": record.degrees,
        "source_generation": record.source_generation,
        "published_generation": record.published_generation,
        "width": record.width,
        "height": record.height,
        "updated_at_ms": record.updated_at_ms,
    }))
}

pub fn render_list(data: &Value) -> String {
    let mut out = String::new();
    let count = data["count"].as_u64().unwrap_or(0);
    let matched = data["matched"].as_u64().unwrap_or(0);
    let total = data["total"].as_u64().unwrap_or(0);
    out.push_str(&format!(
        "{count} of {matched} matched ({total} held; {} waiting, {} synced)\n",
        data["waiting"], data["synced"]
    ));
    for moment in data["moments"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "{:<8} {:<20} {:<24} {}\n",
            moment["state"].as_str().unwrap_or(""),
            moment["form"].as_str().unwrap_or(""),
            moment["entry"].as_str().unwrap_or(""),
            moment["path"].as_str().unwrap_or(""),
        ));
    }
    if data["more"] == true {
        out.push_str("more: raise --limit or narrow the filter\n");
    }
    out
}

pub fn render_read(data: &Value) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(data).unwrap_or_default()
    )
}
