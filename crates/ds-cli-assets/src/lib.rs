//! `ds assets` — Project Assets: the documents a project holds, in folders,
//! with previews, classification and links to the work they belong to.
//!
//! ## Why this domain is a bridge domain
//!
//! An asset's catalogue row lives behind ds-brain, which is the only authority
//! on who may see it: a `restricted` or `confidential` document the caller
//! cannot read has no row at all. Its bytes sit in project storage behind
//! short-lived signed reads; its previews are decoded by the kernel inside the
//! paired application, which also holds the offline catalogue and the content
//! cache; and a file lands on disk through one closed native command of the
//! desktop. None of that is reachable from a file or with an ambient
//! credential, so every command here is one named semantic operation the
//! paired application performs under the session it already holds.
//!
//! The chain is the printing chain: `ds assets …` → the desktop's loopback
//! bridge → the webview executor → the kernel and ds-brain. The Assets tab
//! consumes the same executor, so nothing here is UI-only and nothing in the
//! UI is invisible to the CLI.
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
//! ## What is deliberately absent
//!
//! **An editor.** No command writes asset bytes, under any flag. **A durable
//! link** to anything above `open`. **A second catalogue, uploader or
//! digest** — this surface composes the paths the project already has.

pub mod attach;
pub mod classify;
pub mod folder;
pub mod ingest;
pub mod list;
pub mod preview;
pub mod promote;
pub mod read;
pub mod tree;

use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Domain, Refusal};
use serde_json::{Value, json};

// The paired-application primitives every bridge domain shares. They are
// declared once in `ds-cli-desktop` — the authority surface — so a caller who
// learned `--desktop-descriptor` and the pairing refusals from `ds map` or
// `ds work` has learned them here too.
pub use ds_cli_desktop::ops::{
    AMBIGUOUS, BACKEND_UNREACHABLE, BridgeOp, DESCRIPTOR_ARG, INVALID_NUMBER, NOT_PAIRED, OFFLINE,
    PAIRING_REJECTED, REFUSED, SIGNED_OUT, SIGNED_OUT_MARKERS, UNREACHABLE, UNREADABLE,
    UNSUPPORTED, classify_signed_out, integer, invoke, paired, paired_availability, plural,
};

/// The domain, with its commands in the order a session uses them: find the
/// asset, look at it, then act on it. Domain help prints this order verbatim,
/// so the index doubles as the procedure.
pub static DOMAIN: Domain = Domain {
    id: "assets",
    summary: "Project Assets: documents in folders, previewed and linked.",
    commands: &[
        &list::COMMAND,
        &tree::COMMAND,
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
// The declared wire contract (project-assets-contract.md §7.1, pinned)
// ---------------------------------------------------------------------------

pub const ASSETS_LIST: BridgeOp = BridgeOp {
    operation: "assets.list",
    arguments: &[
        "folder",
        "kind",
        "status",
        "sensitivity",
        "since",
        "limit",
        "cursor",
    ],
};
pub const ASSETS_TREE: BridgeOp = BridgeOp {
    operation: "assets.tree",
    arguments: &["folder", "depth", "into", "query", "link", "kind"],
};
pub const ASSETS_READ: BridgeOp = BridgeOp {
    operation: "assets.read",
    arguments: &["asset", "member", "out"],
};
pub const ASSETS_PREVIEW: BridgeOp = BridgeOp {
    operation: "assets.preview",
    arguments: &["asset", "member", "sheet", "pages", "rows"],
};
pub const ASSETS_CLASSIFY: BridgeOp = BridgeOp {
    operation: "assets.classify",
    arguments: &[
        "asset",
        "kind",
        "status",
        "owner",
        "folder",
        "sensitivity",
        "reason",
    ],
};
pub const ASSETS_PROMOTE: BridgeOp = BridgeOp {
    operation: "assets.promote",
    arguments: &["asset", "member", "as_layer"],
};
pub const ASSETS_ATTACH: BridgeOp = BridgeOp {
    operation: "assets.attach",
    arguments: &["asset", "task", "object_type", "entity_id", "detach"],
};
pub const ASSETS_INGEST: BridgeOp = BridgeOp {
    operation: "assets.ingest",
    arguments: &["path", "folder", "sensitivity"],
};
pub const ASSETS_FOLDER: BridgeOp = BridgeOp {
    operation: "assets.folder",
    arguments: &["path", "sensitivity", "status", "rename_to"],
};

/// Every operation this domain can send, for the parity test to walk. A new
/// operation absent from this list cannot be sent: [`invoke`] takes a
/// [`BridgeOp`], and the test requires each one to be an operation the
/// application actually implements.
pub const BRIDGE_OPS: &[&BridgeOp] = &[
    &ASSETS_LIST,
    &ASSETS_TREE,
    &ASSETS_READ,
    &ASSETS_PREVIEW,
    &ASSETS_CLASSIFY,
    &ASSETS_PROMOTE,
    &ASSETS_ATTACH,
    &ASSETS_INGEST,
    &ASSETS_FOLDER,
];

// ---------------------------------------------------------------------------
// Bounds — hand copies of the contract's, held to the application's adapter
// by `tests/bridge_parity.rs`
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
// Timeouts
// ---------------------------------------------------------------------------

/// A read paints from the application's cached catalogue and reconciles once;
/// a preview may fetch the bytes first.
pub const READ_TIMEOUT: Duration = Duration::from_secs(60);
/// A write is one governed round trip to ds-brain, or one local file write.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(120);
/// An ingest streams the file through the resumable uploader; a field
/// connection decides how long that takes.
pub const INGEST_TIMEOUT: Duration = Duration::from_secs(30 * 60);

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
// Refusals this domain adds to the shared pairing set
// ---------------------------------------------------------------------------

pub const ASSETS_REFUSED: Refusal = Refusal {
    code: "desktop_refused",
    when: "no such asset or folder, or the assets surface declined the command",
    remedy: "check the id with `ds assets list` or `ds assets tree`; read detail.detail for its message",
};

// The application's own structured refusals. The desktop's Project Assets
// adapter mints these from the catalogue's answer and they cross the bridge
// with their class, code and remedy intact, so every one is declared here
// where a caller can look it up; `tests/refusal_coverage.rs` holds the two
// sides equal. Which commands list which follows what the catalogue can say
// to each: a read is never a version conflict.
pub const ASSET_NOT_FOUND: Refusal = Refusal {
    code: "asset_not_found",
    when: "no asset or folder has this id or path, or it is confidential and the caller may not know it exists",
    remedy: "read the available ids and paths with `ds assets list` or `ds assets tree`",
};
pub const ASSET_CLASS_FORBIDDEN: Refusal = Refusal {
    code: "asset_class_forbidden",
    when: "the signed-in user lacks the assets capability this sensitivity class or this write requires",
    remedy: "ask a project admin for the assets capability the message names",
};
pub const ASSET_VERSION_CONFLICT: Refusal = Refusal {
    code: "asset_version_conflict",
    when: "the catalogue row or folder moved while the write was in flight",
    remedy: "re-read it with `ds assets list` or `ds assets tree` and issue the command again",
};
pub const ASSET_REQUEST_INVALID: Refusal = Refusal {
    code: "asset_request_invalid",
    when: "the catalogue declined the request as malformed, or over a bound it names with the number",
    remedy: "change the request as the message says rather than repeating it",
};
pub const ASSET_RULE_REFUSED: Refusal = Refusal {
    code: "asset_refused",
    when: "the catalogue refused the action by one of its rules: a loosening the caller may not make, a promotion that would widen sensitivity, a write onto a system row",
    remedy: "the message names the rule; act on what it names rather than retrying",
};
pub const ASSETS_NOT_IMPLEMENTED: Refusal = Refusal {
    code: "assets_not_implemented",
    when: "the installed application or its catalogue does not serve this action yet",
    remedy: "update DS GridDesign; the action lands later in the Project Assets campaign",
};
pub const ASSETS_SERVICE_FAILED: Refusal = Refusal {
    code: "assets_service_failed",
    when: "the catalogue service faulted while serving the request",
    remedy: "retry once; nothing in the request changes the outcome while the service faults",
};

// The read path's own refusals. `read`, `preview`, `promote` and `tree
// --into` need an asset's bytes, and the application fetches them through
// each source's own read action (§2.5): a projected row that is a summary
// rather than a file, a source with no read action on this surface yet, a
// signed read that expired, or bytes above the read bound are all refused
// by name there, and every name is declared here so a caller can plan for
// it instead of reading `desktop_refused` prose.
pub const ASSET_IS_NOT_A_FILE: Refusal = Refusal {
    code: "asset_is_not_a_file",
    when: "the projected row is a summary, not bytes: a dataset room, a report room, a print setup or a transformer version",
    remedy: "preview it with `ds assets preview`; the summary is the whole of it",
};
pub const ASSET_TOO_LARGE: Refusal = Refusal {
    code: "asset_too_large",
    when: "the bytes are above the read bound the message names with the number",
    remedy: "open it from its own surface, which streams instead of holding it in memory",
};
pub const ORIGIN_READ_FAILED: Refusal = Refusal {
    code: "origin_read_failed",
    when: "the source's own read action returned nothing usable: a row naming no source object, a signed read that expired, or an origin that answered an error",
    remedy: "re-read the row with `ds assets tree` and retry once; a signed read expires quickly by design",
};
pub const ORIGIN_UNREACHABLE: Refusal = Refusal {
    code: "origin_unreachable",
    when: "the source's bytes could not be fetched from this device",
    remedy: "check the connection and retry",
};
pub const ORIGIN_READ_UNAVAILABLE: Refusal = Refusal {
    code: "origin_read_unavailable",
    when: "rows projected from this source carry no read action on this surface yet, DS Grid export outputs among them",
    remedy: "open the row from the surface that owns it; the message names it",
};

// Refusals the adapter constructs from what only it can see: the declared
// folder set, the device's connectivity, a member path against a real
// container, and an asset's kind. Each crosses the bridge under its own
// name; `ds assets` declares the name and the remedy.
pub const UNKNOWN_FOLDER: Refusal = Refusal {
    code: "unknown_folder",
    when: "a folder flag names a path no declared folder has",
    remedy: "declare it with `ds assets folder --path <path>`, or read the declared folders with `ds assets tree`",
};
pub const ASSETS_OFFLINE_WRITE: Refusal = Refusal {
    code: "assets_offline_write",
    when: "this device is offline and the command is a catalogue write",
    remedy: "reconnect, or turn offline mode off with `ds desktop offline set --enabled false`",
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
    when: "--link is not `pm_task:<id>` or `ds_object:<type>:<id>`, or was given more than once",
    remedy: "pass e.g. --link pm_task:t_4812 or --link ds_object:transformer:TX-104",
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
    when: "neither --task nor --object-type with --entity-id was given, or both were",
    remedy: "link to a task with --task, or to a DS object with --object-type and --entity-id",
};
pub const PROJECTED_ASSET_READ_ONLY: Refusal = Refusal {
    code: "projected_asset_read_only",
    when: "a projected `sys:` asset or a system folder was named by classify, attach or folder",
    remedy: "sys: assets cannot be classified, attached or foldered in this slice; act on the source object they project",
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

/// Give the application's refusals the names a caller can plan for.
///
/// The shared rule turns "no active project" prose into `desktop_signed_out`.
/// Everything else this surface refuses — a class the caller lacks, an
/// offline write, a bound with its number — crosses the bridge as the
/// application's own structured refusal, with its class, code and remedy
/// intact, so no prose of this domain's own is keyed on here.
pub fn classify_assets_failure(failure: Failure) -> Failure {
    classify_signed_out(failure)
}

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
    "One member inside a `pack` asset, by the path `ds assets tree --into` reports.",
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
        ["pm_task", id] => !id.is_empty(),
        ["ds_object", object_type, id] => !object_type.is_empty() && !id.is_empty(),
        _ => false,
    };
    if !well_formed {
        return Err(Failure::invalid(
            "invalid_link",
            format!("`--{flag}` must be pm_task:<id> or ds_object:<type>:<id>"),
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
    fn every_declared_operation_is_listed_for_the_parity_test_to_walk() {
        // An operation a handler can send but the list does not carry is one
        // the parity test never proves against the application. The list is
        // the only thing standing between a typo and a runtime refusal.
        let mut names: Vec<&str> = BRIDGE_OPS.iter().map(|op| op.operation).collect();
        names.sort_unstable();
        let mut unique = names.clone();
        unique.dedup();
        assert_eq!(names, unique, "an operation is declared twice");
        assert_eq!(
            names.len(),
            DOMAIN.commands.len(),
            "every ds assets command sends exactly one operation, and every \
             declared operation belongs to a command"
        );
        for command in DOMAIN.commands {
            assert!(
                BRIDGE_OPS.iter().any(|op| op.operation == command.id),
                "`{}` has no operation of the same name",
                command.id
            );
        }
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

    #[test]
    fn the_shared_signed_out_rule_still_applies_and_nothing_else_is_renamed() {
        let refused = |detail: &str| {
            classify_assets_failure(
                Failure::failed("desktop_refused", "refused").detail(json!({ "detail": detail })),
            )
        };
        assert_eq!(
            refused("No active project. Open a project first.").code(),
            "desktop_signed_out"
        );
        assert_eq!(
            refused("asset a_7kq3nr2v0b1c not found").code(),
            "desktop_refused"
        );
    }
}
