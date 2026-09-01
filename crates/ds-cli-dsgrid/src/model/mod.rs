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
pub mod import_external;
pub mod list;
pub mod publish_version;
pub mod set_active;

use std::path::Path;
use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Refusal};
use ds_cli_desktop::ops::BridgeOp;
use serde_json::json;

// The paired-application primitives every bridge domain shares, re-exported
// so this family declares them exactly once and a reader who learned
// `--desktop-descriptor` from `ds map` has learned it here too.
pub use ds_cli_desktop::ops::{
    AMBIGUOUS, DESCRIPTOR_ARG, INVALID_NUMBER, NOT_PAIRED, PAIRING_REJECTED, REFUSED, SIGNED_OUT,
    UNREACHABLE, UNREADABLE, UNSUPPORTED, classify_signed_out, integer, invoke, paired,
    paired_availability, plural,
};

// ---------------------------------------------------------------------------
// The declared wire contract
// ---------------------------------------------------------------------------

pub const MODEL_LIST: BridgeOp = BridgeOp {
    operation: "dsgrid.model.list",
    arguments: &["limit"],
};
pub const MODEL_CREATE: BridgeOp = BridgeOp {
    operation: "dsgrid.model.create",
    arguments: &["name", "crs"],
};
pub const MODEL_IMPORT: BridgeOp = BridgeOp {
    operation: "dsgrid.model.import",
    arguments: &["path", "name"],
};
pub const MODEL_SET_ACTIVE: BridgeOp = BridgeOp {
    operation: "dsgrid.model.set_active",
    arguments: &["model"],
};
pub const MODEL_PUBLISH: BridgeOp = BridgeOp {
    operation: "dsgrid.model.publish",
    arguments: &[
        "model",
        "path",
        "project_model",
        "kind",
        "name",
        "expected_head",
        "reason",
    ],
};

/// Every operation this family can send, for `tests/bridge_parity.rs` to walk.
/// An operation absent from this list is one the parity test never proves
/// against the application.
pub const BRIDGE_OPS: &[&BridgeOp] = &[
    &MODEL_LIST,
    &MODEL_CREATE,
    &MODEL_IMPORT,
    &MODEL_SET_ACTIVE,
    &MODEL_PUBLISH,
];

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

/// Publication captures a snapshot, uploads it and advances a governed head.
pub const PUBLISH_TIMEOUT: Duration = Duration::from_secs(15 * 60);

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
pub const UNSUPPORTED_CRS_MARKERS: &[&str] = &["is not a supported ds grid coordinate system"];
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
    if says(UNSUPPORTED_CRS_MARKERS) {
        return Failure::invalid(
            "unsupported_grid_crs",
            "that coordinate system is not one DS Grid can author against",
        )
        .remedy(UNSUPPORTED_GRID_CRS.remedy);
    }
    if says(EAGER_READ_MARKERS) {
        return Failure::invalid(
            "model_too_large",
            "the .dsgrid package is above the application's eager-read bound",
        )
        .remedy(MODEL_TOO_LARGE.remedy);
    }
    if says(PROJECT_MODEL_MISSING_MARKERS) {
        return Failure::invalid(
            "project_model_not_found",
            "the project catalogue holds no model with that id",
        )
        .remedy(publish_version::PROJECT_MODEL_NOT_FOUND.remedy);
    }
    if says(HEAD_MOVED_MARKERS) {
        return Failure::conflict(
            "publish_head_conflict",
            "the project model's head moved away from the one this call confirmed against",
        )
        .remedy(publish_version::HEAD_CONFLICT.remedy);
    }
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
    format!(
        "  {} {:<24} {:<22} {:<10} {}\n",
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
        row["revision"].as_str().unwrap_or("—"),
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
        let mut names: Vec<&str> = BRIDGE_OPS.iter().map(|op| op.operation).collect();
        names.sort_unstable();
        let mut unique = names.clone();
        unique.dedup();
        assert_eq!(names, unique, "an operation is declared twice");
        assert_eq!(
            names.len(),
            5,
            "the family sends exactly one operation per command"
        );
        assert!(
            names.iter().all(|name| name.starts_with("dsgrid.model.")),
            "every operation belongs to the application's DS Grid model namespace"
        );
    }

    #[test]
    fn no_operation_claims_a_conversion_or_a_project_revision_activation() {
        // The reverted family's mistake, in operation names. `convert` belongs
        // to the DS Grid exchange boundary, and there is no durable exclusive
        // "activate this revision for the project" authority anywhere in this
        // stack — so nothing here may be named as though there were.
        for forbidden in [
            "dsgrid.model.convert",
            "dsgrid.convert",
            "dsgrid.model.activate",
            "dsgrid.project.activate",
            "dsgrid.revision.activate",
            "dsgrid.model.register",
        ] {
            assert!(
                BRIDGE_OPS.iter().all(|op| op.operation != forbidden),
                "`{forbidden}` is exactly the conflation this family exists to keep apart"
            );
        }
    }

    #[test]
    fn no_operation_can_carry_model_content() {
        // Bytes never travel in either direction: a source is a path or an
        // opaque id, and a receipt addresses content by digest and byte count.
        for op in BRIDGE_OPS {
            for argument in op.arguments {
                assert!(
                    !matches!(*argument, "bytes" | "content" | "base64" | "data" | "blob"),
                    "`{}` declares a content-carrying argument `{argument}`",
                    op.operation
                );
            }
        }
    }

    #[test]
    fn no_local_operation_can_carry_a_project() {
        // The load-bearing project-independence claim, held at the wire
        // contract rather than in prose: only `publish` may name project
        // state at all, and even it never names the project itself — the
        // application's own selected project is the destination.
        for op in [&MODEL_LIST, &MODEL_CREATE, &MODEL_IMPORT, &MODEL_SET_ACTIVE] {
            assert!(
                !op.arguments
                    .iter()
                    .any(|argument| argument.contains("project")),
                "`{}` is a local operation and must not carry project state",
                op.operation
            );
        }
        assert!(
            !MODEL_PUBLISH.arguments.contains(&"project"),
            "publication targets the paired session's own selected project; \
             a project id is never an argument"
        );
    }

    #[test]
    fn an_external_source_is_absolute_and_a_dsgrid_before_the_bridge_opens() {
        let absolute = if cfg!(windows) {
            "C:\\models\\route.dsgrid"
        } else {
            "/models/route.dsgrid"
        };
        assert_eq!(
            external_dsgrid_path(absolute, "path").expect("accepted"),
            absolute
        );
        assert_eq!(
            external_dsgrid_path("route.dsgrid", "path")
                .expect_err("must refuse")
                .code(),
            "absolute_path_required"
        );
        // The exchange boundary's sources, refused here by name rather than
        // quietly becoming a second conversion verb.
        for foreign in ["/work/project.bak", "/work/workspace", "/work/line.don"] {
            assert_eq!(
                external_dsgrid_path(foreign, "path")
                    .expect_err("must refuse")
                    .code(),
                "unsupported_model_source",
                "`{foreign}` was accepted as a DS Grid package"
            );
        }
    }

    #[test]
    fn each_named_condition_is_reclassified_only_when_it_really_is_that_condition() {
        let refused = |detail: &str| {
            classify(
                Failure::failed("desktop_refused", "refused").detail(json!({ "detail": detail })),
            )
        };
        assert_eq!(
            refused("No local DS Grid model m-9.").code(),
            "local_model_not_found"
        );
        assert_eq!(
            refused("crs EPSG:4326 is not a supported DS Grid coordinate system.").code(),
            "unsupported_grid_crs"
        );
        assert_eq!(
            refused("The .dsgrid package exceeds the desktop eager-read limit.").code(),
            "model_too_large"
        );
        assert_eq!(
            refused("Project model gm-3 does not exist in ds-project-7. Omit project_model to publish a new one.")
                .code(),
            "project_model_not_found"
        );
        assert_eq!(
            refused("The project head moved: gm-3 is at r-9, not r-7.").code(),
            "publish_head_conflict"
        );
        // Still classified by the shared signed-out rule, and an ordinary
        // application refusal keeps its own identity.
        assert_eq!(
            refused("No active project. Publishing a DS Grid revision needs one; local model operations do not.")
                .code(),
            "desktop_signed_out"
        );
        assert_eq!(
            refused("the model has no rows to capture").code(),
            "desktop_refused"
        );
        assert_eq!(
            classify(Failure::unavailable("desktop_unreachable", "gone")).code(),
            "desktop_unreachable"
        );
    }
}
