//! `ds assets` — Project Assets: the documents a project holds, in folders,
//! with previews, classification and links to the work they belong to.
//!
//! ## Why this domain is headless
//!
//! An asset's catalogue row lives behind ds-brain, which is the only authority
//! on who may see it: a `restricted` or `confidential` document the caller
//! cannot read has no row at all. `POST /api/v1/assets` authenticates from the
//! bearer and resolves the caller's class authority against the project named
//! in the body — no pairing, no device, no window. Its bytes sit in project
//! storage behind short-lived signed reads the same route mints, and enter
//! through the same resumable uploader. So every command here is one governed
//! action the native client sends under the restored user or device
//! credential, for the audience-fenced project named on each command;
//! what the bytes ARE — their format, their members, their preview, the folder
//! tree — is the kernel's decision (`ds_command_kernel::assets`), made here on
//! the host that fetched them. Until 2026-09-20 these nine commands relayed
//! through the paired desktop instead; on a server with no window the owner
//! could file nothing. The window was habit, never contract.
//!
//! ## What the family is
//!
//! ```text
//!   list | tree → read | preview → classify | promote | attach
//!   ingest → folder
//! ```
//!
//! Reads are bounded projections of the one catalogue the Assets tab renders;
//! `preview` returns a document, never pixels. Writes are explicit and
//! confirmed: nothing is ingested by dragging, every classification change is
//! audited, and a link points from the asset to the work — never the other
//! way.
//!
//! ## The index (2026-09-25)
//!
//! ds-brain builds, caches and serves each project's assets index — the
//! catalogued rows plus the `Transformers/`, `MV models/`, `Reports/` … rows
//! it projects from the cloud sources it owns, redacted per caller
//! (ds-brain `docs/contracts/assets-index.md`). `list` reads it as a page in
//! `--order recent` (the timeline) or `name`, `tree` reads it whole, and
//! `versions` reads one row's version history. Every answer says which it is
//! in `index_status`: `served`, or `unavailable` when this lane's ds-brain
//! predates the index — then the answer is the catalogue, as before, and says
//! so. The four edge-only sources (`Local data/`, local `Prints/`) are not in
//! the shared index; `tree` names them in `sources_omitted`.
//!
//! ## What is deliberately absent
//!
//! **An editor.** No command writes asset bytes, under any flag. **A durable
//! link** to anything above `open`. **A second catalogue, uploader or
//! digest** — this surface composes the paths the project already has.
//! **A second projection.** The index is ds-brain's; nothing here projects a
//! source. A projected `sys:` id is listed and versioned, but its bytes are
//! refused by name (`origin_read_unavailable`) — a projected row is read from
//! the surface that owns it. **A window path.** `--desktop-descriptor` is not
//! an input of any `ds assets` command; a caller that still passes it is told
//! `requires_window_retired` by the parser.

pub mod attach;
pub mod backup;
pub mod classify;
pub mod correspondence;
pub mod folder;
pub mod ingest;
pub mod list;
pub mod preview;
pub mod promote;
pub mod read;
pub mod shared;
pub mod tree;
pub mod versions;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Domain, Refusal};
use serde_json::{Value, json};

// Neutral argument helpers: a numeric bound and an English count say
// nothing about a transport, so they come from the contract crate.
pub use ds_cli_contract::args::{INVALID_NUMBER, integer, plural};
pub use ds_client_core::project_assets::Command as CatalogueCommand;

/// The domain, with its commands in the order a session uses them: find the
/// asset, look at it, then act on it. Domain help prints this order verbatim,
/// so the index doubles as the procedure.
pub static DOMAIN: Domain = Domain {
    id: "assets",
    summary: "Project Assets: documents in folders, previewed and linked.",
    commands: &[
        &backup::COMMAND,
        &shared::RESOLVE,
        &shared::MAPS,
        &shared::PUBLISH_MAP,
        &shared::REFERENCE,
        &list::COMMAND,
        &tree::COMMAND,
        &versions::COMMAND,
        &read::COMMAND,
        &preview::COMMAND,
        &classify::COMMAND,
        &promote::COMMAND,
        &attach::COMMAND,
        &ingest::COMMAND,
        &folder::COMMAND,
    ],
};

// ---------------------------------------------------------------------------
// The door
// ---------------------------------------------------------------------------

/// Which native credential lane a `ds assets` command authenticates on.
pub const LANE_ARG: Arg = Arg::value("lane", "<stable|canary>", "Native credential lane.")
    .choices(&["stable", "canary"])
    .default("stable");
pub const PROJECT_ARG: Arg =
    Arg::value("project", "<ds-project>", "Project named for this request.").required();

/// The refusals the headless project client can answer with, for every
/// command of this domain: profile, state, session, identity, transport and
/// project-context conditions. Declared once in `ds auth`.
pub const HEADLESS_REFUSALS: &[Refusal] = ds_cli_auth::PROJECT_STATUS_COMMAND.refusals;
const _: () = assert!(HEADLESS_REFUSALS.len() == 15);

/// The catalogue's own refusals, as the route answers them and `ds auth`
/// maps them (`map_project_assets_refusal`).
pub const CATALOGUE_REFUSALS: [Refusal; 7] = [
    ds_cli_auth::ASSET_NOT_FOUND_REFUSAL,
    ds_cli_auth::ASSET_CLASS_FORBIDDEN_REFUSAL,
    ds_cli_auth::ASSET_VERSION_CONFLICT_REFUSAL,
    ds_cli_auth::ASSET_REQUEST_INVALID_REFUSAL,
    ds_cli_auth::ASSET_REFUSED_REFUSAL,
    ds_cli_auth::ASSETS_NOT_IMPLEMENTED_REFUSAL,
    ds_cli_auth::ASSETS_SERVICE_FAILED_REFUSAL,
];
/// Every command of this domain declares the headless set and the
/// catalogue's, then its own.
pub const BASE: usize = 15 + 7;

/// `TOTAL` is `22 + own.len()`, checked at compile time — const generics
/// cannot add, so the caller states it.
pub const fn refusals<const TOTAL: usize>(own: &[Refusal]) -> [Refusal; TOTAL] {
    assert!(TOTAL == BASE + own.len());
    let mut out = [ASSET_NOT_FOUND; TOTAL];
    let mut i = 0;
    while i < HEADLESS_REFUSALS.len() {
        out[i] = HEADLESS_REFUSALS[i];
        i += 1;
    }
    let mut k = 0;
    while k < CATALOGUE_REFUSALS.len() {
        out[i + k] = CATALOGUE_REFUSALS[k];
        k += 1;
    }
    i += CATALOGUE_REFUSALS.len();
    let mut j = 0;
    while j < own.len() {
        out[i + j] = own[j];
        j += 1;
    }
    out
}

/// One governed catalogue action on the named project.
pub fn catalogue(lane: &str, project: &str, command: &CatalogueCommand) -> Result<Value, Failure> {
    Ok(ds_cli_auth::project_assets_for_project(lane, project, command, None)?.into_result())
}

pub use ds_client_core::project_assets::{
    EDGE_ONLY_SOURCES, INDEX_SERVED, INDEX_UNAVAILABLE, IndexList, IndexOrder,
};

/// Whether an answer is the index (`index_status: "served"`), as the native
/// client marked it — typed, never read from a sentence.
pub fn index_served(answer: &Value) -> bool {
    answer["index_status"] == INDEX_SERVED
}

/// What a person reads when this lane's ds-brain does not serve the index.
pub const INDEX_UNAVAILABLE_NOTICE: &str = "! this lane's ds-brain does not serve the assets index yet: \
     this is the catalogue (uploads only), not the index";

/// The version chip of an index row: `v3 · 12 versions`, or `None` when the
/// row has no versions.
pub fn versions_chip(versions: &Value) -> Option<String> {
    let count = versions["count"].as_u64().filter(|count| *count > 0)?;
    let current = match &versions["current"] {
        Value::Number(number) => Some(format!("v{number}")),
        Value::String(label) if label.parse::<u64>().is_ok() => Some(format!("v{label}")),
        Value::String(label) if !label.trim().is_empty() => Some(label.trim().to_owned()),
        _ => None,
    };
    let count = plural(count, "version");
    Some(match current {
        Some(current) => format!("{} · {count}", truncate(&current, 16)),
        None => count,
    })
}

/// Seconds since the Unix epoch of an RFC 3339 instant: `2026-09-25T09:00:00Z`,
/// fractional seconds and a `±hh:mm` offset allowed. `None` for anything else.
pub fn epoch_seconds(stamp: &str) -> Option<i64> {
    let stamp = stamp.trim();
    if !stamp.is_ascii() {
        return None;
    }
    let (date, time) = stamp.split_once(['T', 't', ' '])?;
    if !calendar_date(date) || !clock_time(time) {
        return None;
    }
    let number = |range: std::ops::Range<usize>, text: &str| text.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (
        number(0..4, date)?,
        number(5..7, date)?,
        number(8..10, date)?,
    );
    let (hour, minute) = (number(0..2, time)?, number(3..5, time)?);
    let second = if time.as_bytes().get(5) == Some(&b':') {
        number(6..8, time)?
    } else {
        0
    };
    let offset = match time.rfind(['+', '-']) {
        Some(at) => {
            let sign = if time.as_bytes()[at] == b'-' { -1 } else { 1 };
            let rest = &time[at + 1..];
            sign * (number(0..2, rest)? * 3_600 + number(3..5, rest)? * 60)
        }
        None => 0,
    };
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second - offset)
}

/// How long before `now` an instant was, in one short word: `just now`,
/// `12m ago`, `3h ago`, `5d ago`, `4mo ago`, `2y ago`. Both are RFC 3339;
/// `None` when either is not.
pub fn ago(then: &str, now: &str) -> Option<String> {
    let seconds = (epoch_seconds(now)? - epoch_seconds(then)?).max(0);
    Some(match seconds {
        0..60 => "just now".to_owned(),
        60..3_600 => format!("{}m ago", seconds / 60),
        3_600..86_400 => format!("{}h ago", seconds / 3_600),
        86_400..2_592_000 => format!("{}d ago", seconds / 86_400),
        2_592_000..31_536_000 => format!("{}mo ago", seconds / 2_592_000),
        _ => format!("{}y ago", seconds / 31_536_000),
    })
}

/// One asset's row and its verified bytes. A projected `sys:` id has no
/// stored bytes the catalogue serves; it is refused by name here.
pub fn bytes(lane: &str, project: &str, asset_id: &str) -> Result<(Value, Vec<u8>), Failure> {
    if is_projected(asset_id) {
        return Err(projected_unavailable(asset_id));
    }
    Ok(ds_cli_auth::read_asset_bytes_for_project(lane, project, asset_id)?.into_result())
}

/// The declared folder at `path`, from the one folder authority.
pub fn folder_at(lane: &str, project: &str, path: &str) -> Result<Value, Failure> {
    let folders = catalogue(lane, project, &CatalogueCommand::Folders)?;
    folders["folders"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|row| row["path"].as_str() == Some(path))
        .cloned()
        .ok_or_else(|| {
            Failure::invalid("unknown_folder", format!("No declared folder at {path}."))
                .remedy(UNKNOWN_FOLDER.remedy)
                .detail(json!({ "path": path }))
                .next(format!("ds assets tree --project {project} --output json"))
        })
}

fn projected_unavailable(asset_id: &str) -> Failure {
    Failure::unavailable(
        "origin_read_unavailable",
        format!(
            "`{asset_id}` is a projected system row; its bytes are served by the surface that owns it, not by the catalogue"
        ),
    )
    .remedy(ORIGIN_READ_UNAVAILABLE.remedy)
    .detail(json!({ "asset": asset_id }))
}

/// The kernel's own refusal of a byte-level request (`evaluate_with_bytes`),
/// as the failure this domain documents: a bound is `asset_too_large`, a
/// member the container does not hold is `invalid_member`, anything else the
/// kernel said is `asset_request_invalid` with its sentence.
pub fn kernel_refused(message: String) -> Failure {
    let lowered = message.to_ascii_lowercase();
    if lowered.contains("bound") {
        Failure::invalid("asset_too_large", message).remedy(ASSET_TOO_LARGE.remedy)
    } else if lowered.contains("member") {
        Failure::invalid("invalid_member", message).remedy(INVALID_MEMBER.remedy)
    } else {
        Failure::invalid("asset_request_invalid", message)
            .remedy(ds_cli_auth::ASSET_REQUEST_INVALID_REFUSAL.remedy)
    }
}

/// Run one kernel request over fetched bytes.
pub fn with_bytes(bytes: &[u8], request: &Value) -> Result<Value, Failure> {
    let request = serde_json::to_vec(request)
        .map_err(|error| Failure::internal("assets_unreadable", error.to_string()))?;
    let answer =
        ds_command_kernel::assets::evaluate_with_bytes(bytes, &request).map_err(kernel_refused)?;
    serde_json::from_str(&answer)
        .map_err(|error| Failure::internal("assets_unreadable", error.to_string()))
}

/// The `assets` request schema every kernel request carries.
pub const REQUEST_SCHEMA: &str = ds_command_kernel::assets::REQUEST_SCHEMA;

/// `sha256:<hex>` of bytes this host holds, spelled as the catalogue spells
/// a digest.
pub fn digest_of(bytes: &[u8]) -> String {
    use sha2::Digest;
    format!("sha256:{:x}", sha2::Sha256::digest(bytes))
}

/// The bytes a kernel answer carried as `bytes_b64`.
pub fn decode_base64(value: &Value) -> Result<Vec<u8>, Failure> {
    use base64::Engine;
    let encoded = value.as_str().ok_or_else(|| {
        Failure::internal(ASSETS_UNREADABLE.code, "the kernel answered no bytes")
            .remedy(ASSETS_UNREADABLE.remedy)
    })?;
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| {
            Failure::internal(ASSETS_UNREADABLE.code, error.to_string())
                .remedy(ASSETS_UNREADABLE.remedy)
        })
}

/// The wire spelling of one closed kernel vocabulary word (`Kind`, `Format`).
pub fn enum_token<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Bounds — the contract's, shared with the kernel and the catalogue
// ---------------------------------------------------------------------------

/// The largest page of catalogue rows one `list` returns. `more` and
/// `next_cursor` say what was cut, so a short page is never silent.
pub const MAX_PAGE_SIZE: i64 = 200;
/// The page a caller gets without asking: the cheapest useful default, and
/// inside the bound — the lesson `ds work` paid for once.
pub const DEFAULT_PAGE_SIZE: i64 = 50;
const _: () = assert!(DEFAULT_PAGE_SIZE <= MAX_PAGE_SIZE);
/// How many folder levels one `tree` read may expand.
pub const MAX_TREE_DEPTH: i64 = 8;
/// The most members one container walk lists before reporting `truncated`.
pub const MAX_CONTAINER_MEMBERS: usize = 5_000;
/// Pages of a paged document one preview includes; the whole document is
/// never rendered eagerly.
pub const MAX_PREVIEW_PAGES: i64 = 5;
/// Rows of a sheet or delimited file one preview includes.
pub const MAX_PREVIEW_ROWS: i64 = 200;
/// The longest `--query` a tree search accepts, in characters.
pub const MAX_QUERY_CHARS: usize = 200;
/// The longest `--as-layer` display name `promote` accepts. Held to the
/// application's own `layerName` bound by the parity suite: a name this CLI
/// refused and the owner would have taken is a round trip that never happened
/// for no reason, and the other way round is a round trip spent to learn a
/// number both sides already knew.
pub const MAX_LAYER_NAME_CHARS: usize = 80;
/// The most links one human projection prints for an asset row before it says
/// how many it left out. A tree read filters on exactly one link.
pub const MAX_LINKS: usize = 32;

// ---------------------------------------------------------------------------
// Vocabularies — the contract's closed words, enforced by the parser
// ---------------------------------------------------------------------------

/// `kind` (§2.3): deliberately short so the words fit a narrow tab.
pub const KINDS: &[&str] = &[
    "doc", "sheet", "mail", "note", "geo", "image", "pack", "other",
];
/// `status` (§2.3, ruled): exactly these three words.
pub const STATUSES: &[&str] = &["fresh", "durable", "archive"];
/// `sensitivity` (§2.4), ordered from open to closed.
pub const SENSITIVITIES: &[&str] = &["open", "internal", "restricted", "confidential"];
/// The two folder kinds a tree read can be narrowed to (§2.5).
pub const FOLDER_KINDS: &[&str] = &["system", "user"];

// ---------------------------------------------------------------------------
// Refusals this domain declares beyond the headless and catalogue sets
// ---------------------------------------------------------------------------

/// The catalogue's codes, re-exported under the names the command files use.
pub const ASSET_NOT_FOUND: Refusal = ds_cli_auth::ASSET_NOT_FOUND_REFUSAL;
pub const ASSET_CLASS_FORBIDDEN: Refusal = ds_cli_auth::ASSET_CLASS_FORBIDDEN_REFUSAL;
pub const ASSET_VERSION_CONFLICT: Refusal = ds_cli_auth::ASSET_VERSION_CONFLICT_REFUSAL;
pub const ASSET_REQUEST_INVALID: Refusal = ds_cli_auth::ASSET_REQUEST_INVALID_REFUSAL;
pub const ASSET_RULE_REFUSED: Refusal = ds_cli_auth::ASSET_REFUSED_REFUSAL;
pub const ASSETS_SERVICE_FAILED: Refusal = ds_cli_auth::ASSETS_SERVICE_FAILED_REFUSAL;
/// A page cursor from an index generation that has since been rebuilt.
pub const ASSETS_INDEX_MOVED: Refusal = ds_cli_auth::ASSETS_INDEX_MOVED_REFUSAL;
pub const ASSETS_INDEX_UNAVAILABLE: Refusal = Refusal {
    code: "assets_index_unavailable",
    when: "this lane's ds-brain predates the assets index, the only holder of version history",
    remedy: "read the row with `ds assets list`; versions arrive with the next ds-brain deployment on this lane",
};
pub const ASSET_BYTES_NOT_HELD: Refusal = Refusal {
    code: "asset_bytes_not_held",
    when: "an external reference has no bytes in DS",
    remedy: "open the external URL in the asset row",
};
pub const INVALID_EXTERNAL_REFERENCE: Refusal = Refusal {
    code: "invalid_external_reference",
    when: "the external URL, digest, size, or provider is invalid",
    remedy: "pass an HTTPS URL, a SHA-256 digest, a positive size, and a valid kind",
};
pub const ASSET_REFERENCE_CONFLICT: Refusal = Refusal {
    code: "asset_reference_conflict",
    when: "a URL and digest already exist under different metadata",
    remedy: "read the existing asset and register changed bytes with their own digest",
};
pub const INVALID_DOCUMENT_REGISTRATION: Refusal = Refusal {
    code: "invalid_document_registration",
    when: "a document number, revision, or state was given without the other two",
    remedy: "pass --document-number, --document-revision and --document-state together",
};
pub const INVALID_REFERENCE_FORM: Refusal = Refusal {
    code: "invalid_reference_form",
    when: "the external or reporter form is incomplete or both were given",
    remedy: "give one complete form; see `ds assets reference --help`",
};
pub const DOCUMENT_STATES: &[&str] = &[
    "draft",
    "issued_for_review",
    "issued_for_approval",
    "issued",
    "approved",
    "superseded",
    "reference",
    "final",
];

pub fn correspondence_door(
    lane: &str,
    project: &str,
    action: &ds_client_core::project_correspondence::Action,
) -> Result<Value, Failure> {
    Ok(
        ds_cli_auth::correspondence::project_correspondence_for_project(lane, project, action)?
            .into_result(),
    )
}

pub fn folder_id(lane: &str, project: &str, path: &str) -> Result<String, Failure> {
    folder_at(lane, project, path)?["folder_id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Failure::internal("assets_unreadable", "folder row has no folder_id"))
}

pub fn digest(raw: &str, flag: &str) -> Result<String, Failure> {
    let trimmed = raw.trim();
    let cleaned = trimmed
        .strip_prefix("sha256:")
        .unwrap_or(trimmed)
        .to_ascii_lowercase();
    if cleaned.len() != 64 || !cleaned.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Failure::invalid(
            INVALID_EXTERNAL_REFERENCE.code,
            format!("`--{flag}` must be a SHA-256 digest of 64 hex characters"),
        )
        .remedy(INVALID_EXTERNAL_REFERENCE.remedy)
        .detail(json!({"field": "sha256", "given": raw})));
    }
    Ok(format!("sha256:{cleaned}"))
}

// The read path's own refusals. `read`, `preview`, `promote` and `tree
// --into` need an asset's bytes, fetched through the catalogue's signed
// read: bytes above the read bound, a signed read that expired, and a
// projected `sys:` row — served by the surface that owns it, not by the
// catalogue — are each refused by name.
pub const ASSET_TOO_LARGE: Refusal = Refusal {
    code: "asset_too_large",
    when: "the bytes are above the read bound the message names with the number",
    remedy: "open it from its own surface, which streams instead of holding it in memory",
};
pub const ORIGIN_READ_FAILED: Refusal = Refusal {
    code: "origin_read_failed",
    when: "the signed read answered nothing usable: it expired, the bytes did not match the row's digest, or the destination could not be written",
    remedy: "re-read the row with `ds assets list` and retry once; a signed read expires quickly by design",
};
pub const ORIGIN_READ_UNAVAILABLE: Refusal = Refusal {
    code: "origin_read_unavailable",
    when: "a projected `sys:` row was named; its bytes are served by the surface that owns it (the application), not by the catalogue",
    remedy: "name a catalogued `a_…` asset; open a projected row from the surface that owns it",
};

// Refusals this domain constructs from what it can see before or after the
// round trip: the declared folder set, a member path against a real
// container, an asset's kind, and the flags themselves.
pub const UNKNOWN_FOLDER: Refusal = Refusal {
    code: "unknown_folder",
    when: "a folder flag names a path no declared folder has",
    remedy: "declare it with `ds assets folder --path <path>`, or read the declared folders with `ds assets tree`",
};
pub const INVALID_MEMBER: Refusal = Refusal {
    code: "invalid_member",
    when: "--member is not a relative path inside the container: it is absolute, or has a `..` segment",
    remedy: "copy the exact member path from `ds assets tree --into <asset>`",
};
pub const ASSET_NOT_GEOGRAPHIC: Refusal = Refusal {
    code: "asset_not_geographic",
    when: "the asset is not a geo asset and no geo member was named",
    remedy: "promote a geo asset, or name a geo member with --member",
};
pub const INVALID_ASSET_ID: Refusal = Refusal {
    code: "invalid_asset_id",
    when: "an asset flag is neither a minted `a_…` id nor a projected `sys:…` id",
    remedy: "copy the exact asset_id from `ds assets list` or `ds assets tree`",
};
pub const INVALID_FOLDER_PATH: Refusal = Refusal {
    code: "invalid_folder_path",
    when: "a folder flag is empty, starts or ends with `/`, or has an empty, `.` or `..` segment",
    remedy: "pass a relative path of named segments, e.g. contracts/2026/epc",
};
pub const INVALID_LINK: Refusal = Refusal {
    code: "invalid_link",
    when: "--link is not `pm_task:<id>`, `pm_record:<id>` or `ds_object:<type>:<id>`, or was given more than once",
    remedy: "pass e.g. --link pm_task:t_4812, --link pm_record:R-0012 or --link ds_object:transformer:TX-104",
};
pub const INVALID_QUERY: Refusal = Refusal {
    code: "invalid_query",
    when: "--query is empty or longer than 200 characters",
    remedy: "pass a short substring; names, paths, kind, format, status and owner are matched",
};
pub const INVALID_DATE: Refusal = Refusal {
    code: "invalid_date",
    when: "--since is not a calendar date in YYYY-MM-DD form or an RFC 3339 timestamp",
    remedy: "pass e.g. --since 2026-09-01 or --since 2026-09-01T00:00:00Z",
};
pub const INVALID_OUT_PATH: Refusal = Refusal {
    code: "invalid_out_path",
    when: "--out is not an absolute path to a new file under an existing directory",
    remedy: "choose a new absolute destination; assets are never written over an existing file",
};
pub const INVALID_SOURCE_PATH: Refusal = Refusal {
    code: "invalid_source_path",
    when: "--path is not an absolute path to an existing, readable file",
    remedy: "pass the absolute path of the file to ingest, e.g. --path /home/me/Documents/lot3.pdf",
};
pub const INVALID_LAYER_NAME: Refusal = Refusal {
    code: "invalid_layer_name",
    when: "--as-layer is empty, longer than 80 characters, or holds a control character",
    remedy: "pass a short display name for the local layer, e.g. --as-layer Lot3-poles",
};
const _: () = assert!(
    MAX_LAYER_NAME_CHARS == 80,
    "INVALID_LAYER_NAME.when states the bound"
);
pub const INVALID_ATTACHMENT: Refusal = Refusal {
    code: "invalid_attachment",
    when: "none of --task, --record or --object-type with --entity-id was given, or more than one was",
    remedy: "link to a task with --task, to a record with --record, or to a DS object with --object-type and --entity-id",
};
pub const PROJECTED_ASSET_READ_ONLY: Refusal = Refusal {
    code: "projected_asset_read_only",
    when: "a projected `sys:` asset or a system folder was named by classify, attach or folder",
    remedy: "sys: assets cannot be classified, attached or foldered; act on the source object they project",
};
/// The kernel could not decode the catalogue's or the file's answer.
pub const ASSETS_UNREADABLE: Refusal = Refusal {
    code: "assets_unreadable",
    when: "the catalogue answered a shape this build cannot fold, or a kernel answer could not be read",
    remedy: "report this with the project id; the CLI and the catalogue disagree about the row",
};
pub const NOTHING_TO_UPDATE: Refusal = Refusal {
    code: "nothing_to_update",
    when: "no classification, owner, folder or sensitivity flag was given",
    remedy: "name at least one change, e.g. --status durable",
};
pub const CONFIRMATION_REQUIRED: Refusal = Refusal {
    code: "confirmation_required",
    when: "--yes was not given for a command that changes the project's catalogue",
    remedy: "re-run with --yes once you intend the change",
};

// ---------------------------------------------------------------------------
// Flag shapes shared across the domain
// ---------------------------------------------------------------------------

pub const ASSET_ARG: Arg = Arg {
    name: "asset",
    kind: ArgKind::Value,
    value: "<asset-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "The asset, by the exact asset_id `ds assets list` or `ds assets tree` reports.",
};

pub const MEMBER_ARG: Arg = Arg::value(
    "member",
    "<path>",
    "One member inside a `pack` asset, or one MIME part of a `mail` (.eml) asset, by the path `ds assets tree --into` reports.",
);

pub const SHEET_ARG: Arg = Arg::value(
    "sheet",
    "<name>",
    "One worksheet of an `xlsx` asset, by the name the preview's sheet list reports; the first sheet otherwise.",
);

pub const FOLDER_ARG: Arg = Arg::value(
    "folder",
    "<path>",
    "A folder path such as contracts/2026/epc.",
);

pub const LIMIT_ARG: Arg = Arg {
    name: "limit",
    kind: ArgKind::Value,
    value: "<count>",
    required: false,
    default: Some("50"),
    choices: &[],
    summary: "Rows in one page (1-200). `more` and `next_cursor` say what was cut.",
};

pub const CURSOR_ARG: Arg = Arg::value(
    "cursor",
    "<token>",
    "Continue from the `next_cursor` a previous page returned.",
);

pub const DEPTH_ARG: Arg = Arg {
    name: "depth",
    kind: ArgKind::Value,
    value: "<levels>",
    required: false,
    default: Some("3"),
    choices: &[],
    summary: "Folder levels to expand (1-8); deeper folders report counts only.",
};

pub const PAGES_ARG: Arg = Arg {
    name: "pages",
    kind: ArgKind::Value,
    value: "<count>",
    required: false,
    default: Some("5"),
    choices: &[],
    summary: "Pages of a paged document to include (1-5); never the whole document.",
};

pub const ROWS_ARG: Arg = Arg {
    name: "rows",
    kind: ArgKind::Value,
    value: "<count>",
    required: false,
    default: Some("200"),
    choices: &[],
    summary: "Rows of a sheet or delimited file to include (1-200).",
};

// ---------------------------------------------------------------------------
// Local validation — typed refusals before a project round trip
// ---------------------------------------------------------------------------

/// An asset flag: a minted id (`a_` and twelve base-36 lowercase characters)
/// or a projected system id (`sys:<source>:<reference>`).
///
/// Checked locally because the two shapes are closed, and a truncated paste
/// is the commonest mistake — it should cost a local refusal, not a project
/// round trip that ends in `desktop_refused`.
pub fn asset_id(raw: &str, flag: &str) -> Result<String, Failure> {
    let trimmed = raw.trim();
    if is_minted(trimmed) || is_projected(trimmed) {
        return Ok(trimmed.to_string());
    }
    Err(
        Failure::invalid("invalid_asset_id", format!("`--{flag}` is not an asset id"))
            .remedy(INVALID_ASSET_ID.remedy)
            .detail(json!({ "given": raw })),
    )
}

/// Whether an id names a projected system row, which `classify`, `attach`
/// and `folder` refuse by name in this slice.
pub fn is_projected(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("sys:") else {
        return false;
    };
    let Some((source, reference)) = rest.split_once(':') else {
        return false;
    };
    !source.is_empty()
        && source
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
        && !reference.is_empty()
}

fn is_minted(id: &str) -> bool {
    id.strip_prefix("a_").is_some_and(|rest| {
        rest.len() == 12
            && rest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte.is_ascii_lowercase())
    })
}

/// The most segments a folder path may have, and the longest one segment.
pub const MAX_FOLDER_SEGMENTS: usize = 8;
pub const MAX_SEGMENT_CHARS: usize = 64;

/// A folder flag: relative, `/`-separated named segments, no traversal.
///
/// Both user folders (`contracts/2026/epc`) and system folders
/// (`Transformers/AGASHARU/reports`) pass; whether the path exists, and
/// whether it may be declared, is the application's answer.
pub fn folder_path(raw: &str, flag: &str) -> Result<String, Failure> {
    let refuse = |why: &str| {
        Failure::invalid("invalid_folder_path", format!("`--{flag}` {why}"))
            .remedy(INVALID_FOLDER_PATH.remedy)
            .detail(json!({ "given": raw }))
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(refuse("is empty"));
    }
    if trimmed.starts_with('/') || trimmed.ends_with('/') {
        return Err(refuse("must not start or end with `/`"));
    }
    if trimmed.contains('\\') || trimmed.chars().any(char::is_control) {
        return Err(refuse("holds a backslash or a control character"));
    }
    let segments: Vec<&str> = trimmed.split('/').collect();
    if segments.len() > MAX_FOLDER_SEGMENTS {
        return Err(refuse(&format!(
            "is nested deeper than {MAX_FOLDER_SEGMENTS} folders"
        )));
    }
    for segment in segments {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(refuse("has an empty, `.` or `..` segment"));
        }
        if segment != segment.trim() {
            return Err(refuse("has a segment with leading or trailing spaces"));
        }
        if segment.chars().count() > MAX_SEGMENT_CHARS {
            return Err(refuse(&format!(
                "has a segment longer than {MAX_SEGMENT_CHARS} characters"
            )));
        }
    }
    Ok(trimmed.to_string())
}

/// A `--link` filter: `pm_task:<id>` or `ds_object:<type>:<id>` (§2.6).
///
/// Exactly those segments, split on every `:` — the same reading the
/// application's adapter makes, so a link this accepts is a link the owner
/// accepts. `pm_task:t_1:extra` is not a task with an odd id; it is a mistake,
/// and it is refused here rather than after a round trip.
pub fn link(raw: &str, flag: &str) -> Result<String, Failure> {
    let trimmed = raw.trim();
    let segments: Vec<&str> = trimmed.split(':').collect();
    let well_formed = match segments.as_slice() {
        ["pm_task", id] | ["pm_record", id] => !id.is_empty(),
        ["ds_object", object_type, id] => !object_type.is_empty() && !id.is_empty(),
        _ => false,
    };
    if !well_formed {
        return Err(Failure::invalid(
            "invalid_link",
            format!("`--{flag}` must be pm_task:<id>, pm_record:<id> or ds_object:<type>:<id>"),
        )
        .remedy(INVALID_LINK.remedy)
        .detail(json!({ "given": raw })));
    }
    Ok(trimmed.to_string())
}

/// A `--query`: a short substring, held to [`MAX_QUERY_CHARS`].
pub fn query(raw: &str, flag: &str) -> Result<String, Failure> {
    let trimmed = raw.trim();
    let chars = trimmed.chars().count();
    if trimmed.is_empty() || chars > MAX_QUERY_CHARS {
        return Err(Failure::invalid(
            "invalid_query",
            format!("`--{flag}` must be 1 to {MAX_QUERY_CHARS} characters"),
        )
        .remedy(INVALID_QUERY.remedy)
        .detail(json!({ "given_chars": chars, "max": MAX_QUERY_CHARS })));
    }
    Ok(trimmed.to_string())
}

/// A `--since` flag: a calendar date, or an RFC 3339 timestamp.
///
/// The date part is checked as a calendar because a transposed day and month
/// is the commonest mistake there is, and it is one the application cannot
/// catch: `2026-01-09` for the ninth of September is a valid date that
/// quietly lists the wrong eight months.
pub fn since(raw: &str, flag: &str) -> Result<String, Failure> {
    let refuse = || {
        Failure::invalid(
            "invalid_date",
            format!("`--{flag}` must be a date in YYYY-MM-DD form, or an RFC 3339 timestamp"),
        )
        .remedy(format!(
            "pass e.g. --{flag} 2026-09-01 or --{flag} 2026-09-01T00:00:00Z"
        ))
        .detail(json!({ "given": raw }))
    };
    let trimmed = raw.trim();
    if !trimmed.is_ascii() {
        return Err(refuse());
    }
    let (date, time) = match trimmed.split_once('T') {
        Some((date, time)) => (date, Some(time)),
        None => (trimmed, None),
    };
    if !calendar_date(date) {
        return Err(refuse());
    }
    if let Some(time) = time
        && !clock_time(time)
    {
        return Err(refuse());
    }
    Ok(trimmed.to_string())
}

fn calendar_date(raw: &str) -> bool {
    let parts: Vec<&str> = raw.split('-').collect();
    if parts.len() != 3 || parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
        return false;
    }
    let mut numbers = [0u32; 3];
    for (slot, part) in numbers.iter_mut().zip(&parts) {
        match part.parse::<u32>() {
            Ok(number) => *slot = number,
            Err(_) => return false,
        }
    }
    let [year, month, day] = numbers;
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    (1970..=2999).contains(&year) && day >= 1 && day <= days_in_month
}

/// `hh:mm[:ss[.fraction]]` followed by `Z` or `±hh:mm`. ASCII only; the
/// caller checked.
fn clock_time(raw: &str) -> bool {
    let (clock, offset) = if let Some(clock) = raw.strip_suffix('Z') {
        (clock, None)
    } else if let Some(at) = raw.rfind(['+', '-']) {
        (&raw[..at], Some(&raw[at + 1..]))
    } else {
        return false;
    };
    if let Some(offset) = offset
        && !hours_minutes(offset)
    {
        return false;
    }
    if clock.len() < 5 {
        return false;
    }
    let (hours_and_minutes, seconds) = clock.split_at(5);
    hours_minutes(hours_and_minutes) && optional_seconds(seconds)
}

fn hours_minutes(raw: &str) -> bool {
    raw.split_once(':')
        .is_some_and(|(hours, minutes)| two_digits(hours, 23) && two_digits(minutes, 59))
}

fn optional_seconds(raw: &str) -> bool {
    if raw.is_empty() {
        return true;
    }
    let Some(rest) = raw.strip_prefix(':') else {
        return false;
    };
    match rest.split_once('.') {
        None => two_digits(rest, 60),
        Some((whole, fraction)) => {
            two_digits(whole, 60)
                && !fraction.is_empty()
                && fraction.bytes().all(|byte| byte.is_ascii_digit())
        }
    }
}

fn two_digits(raw: &str, max: u32) -> bool {
    raw.len() == 2 && raw.parse::<u32>().is_ok_and(|number| number <= max)
}

// ---------------------------------------------------------------------------
// Human projections shared across the domain
// ---------------------------------------------------------------------------

/// Render one catalogue row the same way in every human projection.
pub fn asset_line(row: &Value) -> String {
    let name = row["name"].as_str().unwrap_or("?");
    let path = match row["folder"].as_str().filter(|folder| !folder.is_empty()) {
        Some(folder) => format!("{folder}/{name}"),
        None => name.to_string(),
    };
    format!(
        "  {:<28} {:<5} {:<7} {:<12} {:>10}  {}\n",
        truncate(row["asset_id"].as_str().unwrap_or("?"), 28),
        row["kind"].as_str().unwrap_or("—"),
        row["status"].as_str().unwrap_or("—"),
        row["sensitivity"].as_str().unwrap_or("—"),
        row["bytes"].as_u64().unwrap_or(0),
        truncate(&path, 60),
    )
}

/// Keep a human line one line wide without hiding that it was cut.
pub fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let kept: String = text.chars().take(width.saturating_sub(1)).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_of_the_domain_is_a_headless_project_command_or_a_native_one() {
        // No command of this domain needs a window: the catalogue commands
        // are headless project commands, the shared-reference commands were
        // already native, and the backup plan is local.
        for command in DOMAIN.commands {
            assert_ne!(
                command.requires,
                ds_cli_contract::spec::Requires::Window,
                "`{}` still claims a window",
                command.id
            );
            assert!(
                !command
                    .args
                    .iter()
                    .any(|arg| arg.name == "desktop-descriptor"),
                "`{}` still declares the retired window path",
                command.id
            );
        }
    }

    #[test]
    fn the_refusal_table_holds_the_headless_and_catalogue_sets_first() {
        let table = refusals::<23>(&[INVALID_QUERY]);
        assert_eq!(table[0].code, HEADLESS_REFUSALS[0].code);
        assert_eq!(table[15].code, "asset_not_found");
        assert_eq!(table[21].code, "assets_service_failed");
        assert_eq!(table[22].code, "invalid_query");
        let codes: std::collections::BTreeSet<&str> = table.iter().map(|r| r.code).collect();
        assert_eq!(codes.len(), table.len(), "a code is declared twice");
    }

    #[test]
    fn the_kernels_sentence_becomes_the_code_the_commands_document() {
        assert_eq!(
            kernel_refused(
                "preview input is 40000000 bytes; the preview input bound is 33554432".into()
            )
            .code(),
            "asset_too_large"
        );
        assert_eq!(
            kernel_refused("member gis/poles.shp is not in the container".into()).code(),
            "invalid_member"
        );
        assert_eq!(
            kernel_refused("assets request must use ds.assets.request/v1".into()).code(),
            "asset_request_invalid"
        );
        assert_eq!(
            projected_unavailable("sys:transformers:T1").code(),
            "origin_read_unavailable"
        );
    }

    #[test]
    fn every_default_sits_inside_the_bound_its_summary_states() {
        // `ds work` once shipped a --limit default above its bound, so every
        // call refused. A default is a value the parser will send unasked; it
        // must be one the validator accepts.
        let default = |arg: &Arg| {
            arg.default
                .expect("default")
                .parse::<i64>()
                .expect("number")
        };
        assert_eq!(default(&LIMIT_ARG), DEFAULT_PAGE_SIZE);
        assert!((1..=MAX_TREE_DEPTH).contains(&default(&DEPTH_ARG)));
        assert!((1..=MAX_PREVIEW_PAGES).contains(&default(&PAGES_ARG)));
        assert!((1..=MAX_PREVIEW_ROWS).contains(&default(&ROWS_ARG)));
    }

    #[test]
    fn every_bound_a_summary_or_refusal_states_is_the_constant_it_names() {
        // A number printed in help is a hand copy of a constant, and the
        // parity suite only holds the constants. This holds the prose.
        assert!(LIMIT_ARG.summary.contains("1-200") && MAX_PAGE_SIZE == 200);
        assert!(DEPTH_ARG.summary.contains("1-8") && MAX_TREE_DEPTH == 8);
        assert!(PAGES_ARG.summary.contains("1-5") && MAX_PREVIEW_PAGES == 5);
        assert!(ROWS_ARG.summary.contains("1-200") && MAX_PREVIEW_ROWS == 200);
        assert!(INVALID_QUERY.when.contains("200") && MAX_QUERY_CHARS == 200);
        assert!(INVALID_LAYER_NAME.when.contains("80") && MAX_LAYER_NAME_CHARS == 80);
        assert!(INVALID_FOLDER_PATH.remedy.contains("contracts/2026/epc"));
    }

    #[test]
    fn an_asset_id_is_minted_or_projected_and_nothing_else() {
        for good in [
            "a_7kq3nr2v0b1c",
            "sys:design_attachment:att_1:rev_2",
            "sys:pm_attachment:9",
            "sys:grid_revision:m1:r1",
            " a_000000000000 ",
        ] {
            assert!(asset_id(good, "asset").is_ok(), "`{good}` was refused");
        }
        for bad in [
            "",
            "a_",
            "a_7Kq3nR2v",
            "a_7kq3nr2v0b1",
            "a_7kq3nr2v0b1cd",
            "a-7kq3nr2v0b1c",
            "sys:",
            "sys:design_attachment",
            "sys:design_attachment:",
            "sys:Design:att",
            "sys::att",
            "t_4812",
        ] {
            assert_eq!(
                asset_id(bad, "asset").expect_err("must refuse").code(),
                "invalid_asset_id",
                "`{bad}` was accepted as an asset id"
            );
        }
        assert!(is_projected("sys:report_file:AGASHARU:abc"));
        assert!(!is_projected("a_7kq3nr2v0b1c"));
    }

    #[test]
    fn a_folder_path_is_named_segments_without_traversal() {
        assert_eq!(
            folder_path(" contracts/2026/epc ", "folder").expect("valid"),
            "contracts/2026/epc"
        );
        assert!(folder_path("Transformers/AGASHARU/reports", "folder").is_ok());
        assert!(folder_path("MV models/Feeder 3", "folder").is_ok());
        for bad in [
            "",
            "/contracts",
            "contracts/",
            "contracts//epc",
            "contracts/./epc",
            "../contracts",
            "contracts\\epc",
            "contracts/ epc",
            "a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q",
        ] {
            assert_eq!(
                folder_path(bad, "folder").expect_err("must refuse").code(),
                "invalid_folder_path",
                "`{bad}` was accepted as a folder path"
            );
        }
    }

    #[test]
    fn a_link_names_a_task_or_a_ds_object() {
        assert!(link("pm_task:t_4812", "link").is_ok());
        assert!(link("ds_object:transformer:TX-104", "link").is_ok());
        for bad in [
            "",
            "pm_task",
            "pm_task:",
            "pm_task:t_4812:extra",
            "ds_object:transformer",
            "ds_object::TX-104",
            "ds_object:transformer:",
            "ds_object:transformer:TX-104:extra",
            "PM_TASK:t_4812",
            "task:t_4812",
        ] {
            assert_eq!(
                link(bad, "link").expect_err("must refuse").code(),
                "invalid_link",
                "`{bad}` was accepted as a link"
            );
        }
    }

    #[test]
    fn a_query_is_short_and_not_empty() {
        assert_eq!(query(" poles ", "query").expect("valid"), "poles");
        assert!(query(&"x".repeat(MAX_QUERY_CHARS), "query").is_ok());
        for bad in ["", "   ", &"x".repeat(MAX_QUERY_CHARS + 1)] {
            assert_eq!(
                query(bad, "query").expect_err("must refuse").code(),
                "invalid_query"
            );
        }
    }

    #[test]
    fn an_index_time_reads_as_how_long_ago_it_was() {
        assert_eq!(epoch_seconds("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(epoch_seconds("2026-09-25T09:00:00Z"), Some(1_790_326_800));
        assert_eq!(
            epoch_seconds("2026-09-25T11:00:00.750+02:00"),
            epoch_seconds("2026-09-25T09:00:00Z")
        );
        assert_eq!(epoch_seconds("2026-09-25T09:00Z"), Some(1_790_326_800));
        for bad in [
            "",
            "2026-09-25",
            "2026-13-01T00:00:00Z",
            "yesterday",
            "2026-09-25T09:00:00",
        ] {
            assert_eq!(epoch_seconds(bad), None, "`{bad}` read as a time");
        }
        let now = "2026-09-25T10:00:00Z";
        for (then, expected) in [
            ("2026-09-25T09:59:30Z", "just now"),
            ("2026-09-25T09:48:00Z", "12m ago"),
            ("2026-09-25T07:00:00Z", "3h ago"),
            ("2026-09-20T10:00:00Z", "5d ago"),
            ("2026-05-25T10:00:00Z", "4mo ago"),
            ("2024-09-25T10:00:00Z", "2y ago"),
            // A clock a little ahead of the index's is not the future.
            ("2026-09-25T10:00:09Z", "just now"),
        ] {
            assert_eq!(ago(then, now).as_deref(), Some(expected), "{then}");
        }
        assert_eq!(ago("not a time", now), None);
    }

    #[test]
    fn a_versions_chip_names_the_current_version_and_the_count() {
        assert_eq!(
            versions_chip(&json!({"count": 12, "current": "v3"})).as_deref(),
            Some("v3 · 12 versions")
        );
        assert_eq!(
            versions_chip(&json!({"count": 1, "current": 1})).as_deref(),
            Some("v1 · 1 version")
        );
        assert_eq!(
            versions_chip(&json!({"count": 2, "current": "7"})).as_deref(),
            Some("v7 · 2 versions")
        );
        assert_eq!(
            versions_chip(&json!({"count": 4})).as_deref(),
            Some("4 versions")
        );
        assert_eq!(versions_chip(&json!({"count": 0})), None);
        assert_eq!(versions_chip(&Value::Null), None);
    }

    #[test]
    fn a_since_flag_is_a_calendar_date_or_a_timestamp() {
        for good in [
            "2026-09-01",
            "2028-02-29",
            "2026-09-01T00:00Z",
            "2026-09-01T08:12:00Z",
            "2026-09-01T08:12:00.250Z",
            "2026-09-01T08:12:00+02:00",
            "2026-09-01T08:12-05:00",
        ] {
            assert!(since(good, "since").is_ok(), "`{good}` was refused");
        }
        for bad in [
            "",
            "2026-9-1",
            "01-09-2026",
            "2026-13-01",
            "2026-02-29",
            "tomorrow",
            "2026-09-01T",
            "2026-09-01T08:12",
            "2026-09-01T25:00Z",
            "2026-09-01T08:60Z",
            "2026-09-01T08:12:00.Z",
            "2026-09-01T08:12:00+2:00",
            "2026-09-01Tü8:12Z",
        ] {
            assert_eq!(
                since(bad, "since").expect_err("must refuse").code(),
                "invalid_date",
                "`{bad}` was accepted as a --since value"
            );
        }
    }
}
