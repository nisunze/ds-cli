//! `ds pls` — PLS-CADD workspaces and structures.
//!
//! Every command here is one of `ds-grid-tasks`' typed file tasks. That crate
//! exists precisely so a host does not have to know how a `.don`, a `.012` or
//! a workspace's reference closure is read: it takes a typed request, loads
//! the exact bytes, checks identity, calls the owning operation, and returns a
//! bounded typed result.
//!
//! So this domain is thin on purpose. It translates declared flags into a
//! task request and shapes the answer. It parses no PLS format, resolves no
//! reference, and compares no station — those live behind the task boundary,
//! and a second implementation of any of them here would be a second answer
//! to a question that must have one.
//!
//! The tasks also publish their own request schemas. Where a request is too
//! structured for flags, the schema is what a caller reads and
//! `--request <file>` is what they pass, rather than the domain growing a
//! flag per nested field.

pub mod compare_don;
pub mod delivery_verify;
pub mod deviation_labels;
pub mod pole_capacity;
pub mod reference_closure;
pub mod section_orientation;
pub mod terrain_reconcile;

use ds_cli_contract::spec::Domain;

pub static DOMAIN: Domain = Domain {
    id: "pls",
    summary: "PLS-CADD workspaces: structures, capacity, references, DONs.",
    commands: &[
        &pole_capacity::COMMAND,
        &reference_closure::COMMAND,
        &section_orientation::COMMAND,
        &compare_don::COMMAND,
        &terrain_reconcile::COMMAND,
        &deviation_labels::COMMAND,
        &delivery_verify::COMMAND,
    ],
};

use std::path::PathBuf;

use ds_cli_contract::outcome::Failure;
use serde_json::json;

/// Resolve a path argument to an existing file, as an absolute path.
///
/// Two reasons this is not just a `PathBuf::from`:
///
/// * The tasks **refuse a relative path**. Making the caller write an
///   absolute one would be a needless sharp edge, so `ds` resolves it.
/// * The tasks also refuse an unreadable source, with their own codes.
///   Checking first is not duplication: it turns "the task refused, read its
///   nested detail" into "that path is not a file", which is the difference
///   between fixing a typo and reading an error inside an error.
pub fn source_path(raw: &str, flag: &str) -> Result<PathBuf, Failure> {
    let path = PathBuf::from(raw);
    if !path.is_file() {
        return Err(
            Failure::invalid("source_not_found", format!("`{raw}` is not a file"))
                .remedy(format!("check the path passed to --{flag}")),
        );
    }
    path.canonicalize().map_err(|error| {
        Failure::invalid(
            "source_not_found",
            format!("`{raw}` could not be resolved to an absolute path"),
        )
        .remedy(format!("check the path passed to --{flag}"))
        .detail(json!({ "detail": error.kind().to_string() }))
    })
}

/// Resolve a workspace directory without weakening the task's absolute-path
/// contract.
pub fn workspace_path(raw: &str) -> Result<PathBuf, Failure> {
    let path = PathBuf::from(raw);
    if !path.is_dir() {
        return Err(
            Failure::invalid("workspace_not_found", format!("`{raw}` is not a directory"))
                .remedy("pass the closed PLS-CADD workspace root"),
        );
    }
    path.canonicalize().map_err(|error| {
        Failure::invalid(
            "workspace_not_found",
            format!("`{raw}` could not be resolved: {error}"),
        )
    })
}

/// Resolve an absent output to an absolute path while preserving its new leaf.
pub fn output_path(raw: &str) -> Result<PathBuf, Failure> {
    let path = PathBuf::from(raw);
    if path.exists() {
        return Err(
            Failure::conflict("output_exists", format!("`{raw}` already exists"))
                .remedy("choose a new immutable workspace path"),
        );
    }
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map_err(|error| Failure::failed("output_write_failed", error.to_string()))?
            .join(path)
    };
    Ok(absolute)
}

/// The documented refusal for a result that will not encode.
///
/// Reachable, if narrowly: `serde_json` refuses a non-finite float, and these
/// results carry engineering values read from a source document. It is
/// declared by every command in this domain rather than exempted, because
/// "the task answered and ds could not render it" is a real thing a caller
/// can hit and must be able to recognise.
pub const RESULT_ENCODING_REFUSAL: ds_cli_contract::spec::Refusal =
    ds_cli_contract::spec::Refusal {
        code: "result_unserializable",
        when: "the task answered but its result could not be encoded",
        remedy: "the source likely carries a non-finite value; report it with the source digest",
    };

/// Encode a task result, or refuse with [`RESULT_ENCODING_REFUSAL`]'s code.
pub fn encode<T: serde::Serialize>(result: &T) -> Result<serde_json::Value, Failure> {
    serde_json::to_value(result).map_err(|error| {
        Failure::failed(
            "result_unserializable",
            "the task answered but its result could not be encoded",
        )
        .remedy("the source likely carries a non-finite value; report it with the source digest")
        .detail(json!({ "detail": error.to_string() }))
    })
}

/// Validate a paging limit against the bound the task itself publishes.
///
/// The tasks bound their limits tightly — 64 items for a capacity block, 32
/// for a reference closure — and refuse anything larger. Copying those
/// numbers here is the hand-copy that drifts: the first version of this
/// domain defaulted to 50 and every `ds pls reference-closure` call failed
/// with the task's own `invalid_limit`, because 50 is over its bound of 32.
///
/// So the bound is read from the request schema the task publishes, and the
/// refusal quotes it. A bound that changes upstream changes here with it.
pub fn bounded_limit(
    raw: Option<&str>,
    schema: &serde_json::Value,
    field: &str,
) -> Result<usize, Failure> {
    let spec = &schema["properties"][field];
    let maximum = spec["maximum"].as_u64().unwrap_or(u64::MAX) as usize;
    let minimum = spec["minimum"].as_u64().unwrap_or(0) as usize;
    let default = spec["default"].as_u64().map(|value| value as usize);

    let Some(raw) = raw else {
        return Ok(default.unwrap_or(minimum.max(1)));
    };
    let parsed: usize = raw.parse().map_err(|_| {
        Failure::invalid(
            "invalid_number",
            format!("`--{field}` must be a whole number"),
        )
        .remedy(format!("pass {minimum}..{maximum}"))
    })?;
    if parsed < minimum.max(1) || parsed > maximum {
        return Err(Failure::invalid(
            "invalid_number",
            format!("`--{field}` is outside the task's bound"),
        )
        .remedy(format!("pass {}..{maximum}", minimum.max(1)))
        .detail(json!({ "given": parsed, "min": minimum.max(1), "max": maximum })));
    }
    Ok(parsed)
}

/// The canonical `sha256:<64 hex>` digest of a file's exact bytes.
///
/// Offered so a caller can *obtain* a pin without shelling out to
/// `sha256sum`. It does not weaken the pin: the task still recomputes and
/// compares at run time, which is the whole point — the digest is recorded
/// when a decision is made and re-checked when the work runs.
pub fn file_digest(path: &std::path::Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Some(format!("sha256:{:x}", hasher.finalize()))
}

/// A whole-number flag, or its declared default.
pub fn numeric(raw: Option<&str>, default: usize) -> Result<usize, Failure> {
    let Some(raw) = raw else { return Ok(default) };
    raw.parse().map_err(|_| {
        Failure::invalid("invalid_number", format!("`{raw}` is not a whole number"))
            .remedy("pass a whole number; counts and offsets start at 0")
    })
}

/// Carry a task's own refusal through without reinterpreting it.
///
/// The task's `code` goes into `detail` rather than becoming `error.code`.
/// That is deliberate: `ds` documents the codes *it* can emit, and a task's
/// vocabulary is its own and may grow without notice. A caller branching on
/// `task_refused` and reading `detail.code` sees both, and the `ds` contract
/// stays a closed set.
pub fn task_failure(code: &str, detail: &str) -> Failure {
    Failure::failed("task_refused", "the task refused this request")
        .remedy("read detail.code and detail.detail for the task's own reason")
        .detail(json!({
            "code": code,
            "detail": detail.chars().take(400).collect::<String>(),
        }))
}
