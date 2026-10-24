//! The DS GridDesign-local model lifecycle, and the one project act that
//! publishes a revision of one.
//!
//! ## Why this family is a bridge family
//!
//! The rest of `ds dsgrid` reads a `.dsgrid` file the caller already has, so
//! it links `ds-grid-model` and computes nothing. A *local model* is a
//! different thing entirely: it is a live worker session and a durable store
//! inside the running application, and it is the application's own session —
//! not a file — that decides which one occupies Profile and editing. There is
//! no file to read and no headless owner to link, so every command here is one
//! named semantic operation the paired application performs.
//!
//! ## The three concepts this family keeps apart
//!
//! A provisional `ds dsgrid model create/import/convert` family was reverted
//! before release because it folded all three into one word. The paired
//! application's own contract — `docs/dsgrid-local-model-and-project-publication-contract.md`
//! in `ds-web` — names them apart, and the command wording here says the same
//! thing out loud:
//!
//! | Concept | Command | Scope |
//! |---|---|---|
//! | Acquisition — a new empty model, or an external `.dsgrid` | `create-local`, `import-external` | local |
//! | Local active — the ONE session occupying Profile and editing | `set-active` | local |
//! | Project publication — ONE immutable revision in the catalogue | `publish-version` | project |
//!
//! Four properties are load-bearing, and each has a negative control in
//! `crates/ds/tests/domain_smoke.rs`:
//!
//! * **The local family is project-independent.** `list`, `create-local`,
//!   `import-external` and `set-active` neither require nor accept a project.
//!   Only `publish-version` reads one, because only it resolves an exact
//!   catalogue revision — which is why it alone lives outside `ds dsgrid
//!   model` and carries `Authority::Project`.
//! * **Publication implies no local activation.** There is no durable
//!   exclusive "activate this revision for the project" authority anywhere in
//!   this stack, so nothing here pretends to one. The publish receipt reports
//!   `active_model` and `active_model_changed` so "published" can never be
//!   read as "now current".
//! * **No model bytes travel.** A source is a filesystem path the shell
//!   prepares, or an opaque local model id. Receipts carry digests, revisions
//!   and byte *counts*.
//! * **Import is `.dsgrid` only.** A PLS-CADD workspace or `.bak` is a
//!   conversion source and belongs to `ds dsgrid-exchange inspect | plan |
//!   convert`. A second convert-and-project verb is exactly the conflation the
//!   revert reversed, so this family refuses one locally and says where it
//!   went.

pub mod create_local;
pub mod forget;
pub mod import_external;
pub mod link;
pub mod list;
pub mod pls_source;
pub mod prepare_project;
pub mod set_active;
pub mod show;
pub mod unlink;
pub mod workspace;

use std::path::Path;
use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Refusal};
use ds_cli_desktop::ops::BridgeOp;
use serde_json::json;

// The paired-application primitives every bridge domain shares, re-exported
// so this family declares them exactly once and a reader who learned
// `--desktop-descriptor` from `ds map` has learned it here too.
// Neutral argument helpers: a numeric bound and an English count say
// nothing about a paired window, so they come from the contract crate.
pub use ds_cli_contract::args::{INVALID_NUMBER, integer, plural};
pub use ds_cli_desktop::ops::{
    AMBIGUOUS, DESCRIPTOR_ARG, NOT_PAIRED, PAIRING_REJECTED, PROJECT_NOT_OPEN, REFUSED, SIGNED_OUT,
    TARGET_ARG, UNREACHABLE, UNREADABLE, UNSUPPORTED, classify_signed_out, invoke, paired,
    paired_availability,
};

// ---------------------------------------------------------------------------
// The declared wire contract
// ---------------------------------------------------------------------------

/// Every operation this family can send, for `tests/bridge_parity.rs` to walk.
/// An operation absent from this list is one the parity test never proves
/// against the application.
/// What this domain can still send to the paired application: the working
/// copy the application itself holds open. Model management left this door
/// on 2026-09-18 — a working copy is a fact about a machine, so `ds dsgrid
/// model list|create-local|import-external|set-active` answer from this
/// machine's own catalogue — and `prepare-project` left it on 2026-09-20:
/// readiness is a fact about this machine's catalogue against the project's
/// governed heads, and a missing head is downloaded through the same door
/// `ds dsgrid project download` uses.
/// `dsgrid.profile.open` (2026-09-20) is the one way back: the application
/// opens this machine's working copy by path, under the copy's own id, and
/// occupies Profile with it. `dsgrid.model.publish` left with its sender,
/// `publish-version`, in b864f988 (DECOUPLING-003) and the application no
/// longer admits it.
pub const BRIDGE_OPS: &[&BridgeOp] = &[&crate::profile::open::PROFILE_OPEN];

/// The largest page of local models one read returns. A hand copy of the
/// adapter's own `MAX_LIST_LIMIT`, held to it by `tests/bridge_parity.rs`; the
/// total is always reported, so a truncated page is never silent.
pub const MAX_LIST_LIMIT: i64 = 500;

/// The cheapest useful default. A workstation holds a handful of local models,
/// not hundreds, and `more` reports anything withheld.
pub const DEFAULT_LIST_LIMIT: &str = "50";

/// The project catalogue's model kinds. A hand copy of the application's
/// `GridModelKind`, held to its source by `tests/bridge_parity.rs`.
pub const MODEL_KINDS: &[&str] = &["general", "lv_network", "mv_line"];

/// Opening a worker, decoding a package and reaching a first checkpoint are
/// all local, but none of them is instant on a field laptop.
pub const LOCAL_TIMEOUT: Duration = Duration::from_secs(5 * 60);

// ---------------------------------------------------------------------------
// Flag shapes shared across the family
// ---------------------------------------------------------------------------

pub const MODEL_ARG: Arg = Arg {
    name: "model",
    kind: ArgKind::Value,
    value: "<model-id>",
    required: false,
    default: None,
    choices: &[],
    summary: "A local model, by the opaque id `ds dsgrid model list` reports.",
};

pub const NAME_ARG: Arg = Arg {
    name: "name",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "Display name. Never becomes an id.",
};

// ---------------------------------------------------------------------------
// Refusals this family adds to the shared pairing set
// ---------------------------------------------------------------------------

pub const ABSOLUTE_PATH_REQUIRED: Refusal = Refusal {
    code: "absolute_path_required",
    when: "--path is not an absolute path",
    remedy: "resolve the .dsgrid path on the DS GridDesign machine before invoking",
};
pub const UNSUPPORTED_MODEL_SOURCE: Refusal = Refusal {
    code: "unsupported_model_source",
    when: "--path does not name a .dsgrid package",
    remedy: "convert a PLS-CADD workspace or .bak first with `ds dsgrid-exchange plan|convert`",
};
pub const MODEL_TOO_LARGE: Refusal = Refusal {
    code: "model_too_large",
    when: "the .dsgrid package is above the application's eager-read bound",
    remedy: "open the package in DS GridDesign directly; it is too large for the bridge",
};
pub const LOCAL_MODEL_NOT_FOUND: Refusal = Refusal {
    code: "local_model_not_found",
    when: "no local model has that id",
    remedy: "list the ids with `ds dsgrid model list`",
};
pub const UNSUPPORTED_GRID_CRS: Refusal = Refusal {
    code: "unsupported_grid_crs",
    when: "--crs is not a coordinate system DS Grid can author against",
    remedy: "pass a supported projected metric CRS, e.g. EPSG:32735, or omit --crs",
};
pub const AUTH_CONTEXT_MISMATCH: Refusal = Refusal {
    code: "auth_context_mismatch",
    when: "the paired session published no complete identity, or it changed mid-call",
    remedy: "sign in to DS GridDesign on the matching lane and retry",
};

/// What the application says for each condition this family gives its own
/// code. Hand copies of its prose — the least stable thing to key on — so
/// `tests/bridge_parity.rs` requires every marker to still appear in the
/// adapter's source, and an unmatched refusal stays the untranslated one
/// rather than a wrong one.
pub const LOCAL_MODEL_MISSING_MARKERS: &[&str] = &["no local ds grid model"];
pub const EAGER_READ_MARKERS: &[&str] = &["exceeds the desktop eager-read limit"];
pub const PROJECT_MODEL_MISSING_MARKERS: &[&str] = &["does not exist in"];
pub const HEAD_MOVED_MARKERS: &[&str] = &["the project head moved"];

/// Give this family's named conditions their own codes.
///
/// Each arrives as an ordinary operation refusal — the application answered,
/// and what it answered has a name, a remedy and a different next step. A
/// caller that cannot tell "that id is not a local model" from "the project
/// head moved under you" either retries forever or abandons a call that would
/// have worked.
pub fn classify(failure: Failure) -> Failure {
    let failure = classify_signed_out(failure);
    if failure.code() != "desktop_refused" {
        return failure;
    }
    let detail = failure
        .detail_value()
        .and_then(|detail| detail["detail"].as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let says = |markers: &[&str]| markers.iter().any(|marker| detail.contains(marker));

    if says(LOCAL_MODEL_MISSING_MARKERS) {
        return Failure::invalid("local_model_not_found", "no local model has that id")
            .remedy(LOCAL_MODEL_NOT_FOUND.remedy)
            .next("ds dsgrid model list");
    }
    // An unsupported coordinate system was the application's answer to
    // `create`, which no longer crosses this door: the engine linked into `ds`
    // answers it directly now, so there is no prose left to translate.
    if says(EAGER_READ_MARKERS) {
        return Failure::invalid(
            "model_too_large",
            "the .dsgrid package is above the application's eager-read bound",
        )
        .remedy(MODEL_TOO_LARGE.remedy);
    }
    // `project_model_not_found` (PROJECT_MODEL_MISSING_MARKERS) and
    // `publish_head_conflict` (HEAD_MOVED_MARKERS) name what the desktop's
    // publication answers; they return here with `ds dsgrid publish-version`
    // (Architecture DECOUPLING-003). `profile open` never receives them.
    failure
}

// ---------------------------------------------------------------------------
// Local input validation
// ---------------------------------------------------------------------------

/// One external `.dsgrid` source, checked here rather than one round trip
/// later.
///
/// Two properties, and both are this repository's rather than the
/// application's. **Absolute**, because the application resolves the path in
/// its own working directory and a relative one silently means a different
/// file. **`.dsgrid`**, because a PLS-CADD workspace or `.bak` is a conversion
/// source owned by `ds dsgrid-exchange` — admitting one here would be the
/// second convert-and-project verb the reverted family was reverted for.
pub fn external_dsgrid_path(raw: &str, flag: &str) -> Result<String, Failure> {
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err(Failure::invalid(
            "absolute_path_required",
            format!("`--{flag}` must be an absolute path on the DS GridDesign machine"),
        )
        .remedy(ABSOLUTE_PATH_REQUIRED.remedy)
        .detail(json!({ "given": raw })));
    }
    let is_dsgrid = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("dsgrid"));
    if !is_dsgrid {
        return Err(Failure::invalid(
            "unsupported_model_source",
            format!("`--{flag}` must name a .dsgrid package"),
        )
        .remedy(UNSUPPORTED_MODEL_SOURCE.remedy)
        .next("ds dsgrid-exchange inspect --source <path>")
        .detail(json!({ "given": raw })));
    }
    Ok(raw.to_string())
}

/// Render one local model row the same way in every human projection.
pub fn model_line(row: &serde_json::Value, active: &str) -> String {
    let id = row["model"].as_str().unwrap_or("?");
    let mut line = format!(
        "  {} {:<24} {:<22} {:<10} rev {:<4} {}\n",
        if id == active { "*" } else { " " },
        truncate(id, 24),
        truncate(row["name"].as_str().unwrap_or(""), 22),
        row["status"]
            .as_str()
            .unwrap_or(if row["open"].as_bool().unwrap_or(false) {
                "open"
            } else {
                "idle"
            }),
        row["revision"],
        row["head_revision"]
            .as_str()
            .map(crate::mutation::short)
            .unwrap_or_else(|| "—".to_string()),
    );
    // A linked copy says where a sync would write, under its own line: the
    // path is long and the table columns are not the place for it.
    if let Some(link) = row["pls_source"].as_object() {
        line.push_str(&format!(
            "      ↳ PLS-CADD {} · {} · {}\n",
            link.get("pls_version")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?"),
            link.get("path")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?"),
            link.get("digest")
                .and_then(serde_json::Value::as_str)
                .map(|digest| truncate(digest, 19))
                .unwrap_or_default(),
        ));
    }
    line
}

/// Keep a human line one line wide without hiding that it was cut.
pub use ds_cli_contract::util::truncate;

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
