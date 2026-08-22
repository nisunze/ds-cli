//! `ds network convert inspect` — what are these files, and what can be done
//! with them?
//!
//! This is the first question anyone has about a pile of engineering data,
//! and until now the only way to answer it was to try a conversion and read
//! the failure. It classifies each source, records its exact digest, and
//! returns the engine's own capability matrix: which conversions are
//! available from this set, which are blocked, and why.
//!
//! It is read-only and writes nothing. Nothing is converted, and no output
//! path is touched — the point is to decide *whether* to convert before doing
//! it.
//!
//! The classification and the capability matrix are both the engine's. `ds`
//! reads bytes, hands them over, and shapes the answer.

use std::path::{Path, PathBuf};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_exchange::conversion::{
    CapabilityState, SourceCandidate, SourceSet, conversion_capabilities, inspect_sources,
};
use serde_json::{Value, json};

/// A folder source is read whole. The bound is the same one the reference
/// closure task applies to a workspace, for the same reason: a mistyped path
/// at a large tree should fail in a moment, not after reading it.
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
const MAX_FILES: usize = 4_096;

pub static COMMAND: Command = Command {
    id: "network.convert.inspect",
    path: &["network", "convert", "inspect"],
    contract: 1,
    summary: "Classify source files and report what can be converted from them.",
    purpose: "\
Answers what a pile of engineering files actually is, and what the engine can \
do with it. Each source is classified and digested; the result carries the \
capability matrix — every conversion this set supports, every one it does not, \
and the reason. Nothing is converted and nothing is written: this is the call \
that decides whether a conversion is worth attempting.",
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::repeated(
            "source",
            "<path>",
            "A file, or a directory read as one folder source. Repeatable.",
        )
        .required(),
        Arg::switch(
            "blocked",
            "List blocked capabilities too, with their reasons.",
        ),
    ],
    output: "\
One entry per source with its classification, digest, member count and any \
version or units evidence the engine recovered; then the capabilities, \
available ones by default.",
    examples: &[
        Example {
            command: "ds network convert inspect --source ./workspace --output json",
            note: "A PLS-CADD workspace directory, read as one folder source.",
            runnable: false,
        },
        Example {
            command: "ds network convert inspect --source ./a.dsgrid --source ./b.dsgrid --blocked --output json",
            note: "Several sources at once, with the reasons nothing else is offered.",
            runnable: false,
        },
    ],
    refusals: &[
        Refusal {
            code: "source_not_found",
            when: "a --source path does not exist",
            remedy: "check each path; a directory is read as one folder source",
        },
        Refusal {
            code: "source_too_large",
            when: "the sources exceed the 512 MiB or 4096-file read bound",
            remedy: "inspect a narrower subtree",
        },
        Refusal {
            code: "source_unreadable",
            when: "a source exists but cannot be read",
            remedy: "check file permissions",
        },
    ],
    reference: Some("docs/reference/network.md"),
    availability: available,
};

fn available() -> Availability {
    Availability::Available
}

/// Whether a capability is worth offering by default.
///
/// Matched on the enum, not on its `Debug` spelling. An earlier version
/// compared `format!("{:?}", state)` against `"Available"` — a variant that
/// does not exist — so every capability was filtered out and a source set
/// with six ready conversions reported "none available". A stringly-typed
/// comparison against a name nobody checked is how that happens.
///
/// `Unverified` is included deliberately: a path that exists but has not been
/// verified for these inputs is something a caller may want to attempt, and
/// its reason says so. `Unsupported` and `NotImplemented` are not offers.
const fn offerable(state: CapabilityState) -> bool {
    matches!(state, CapabilityState::Ready | CapabilityState::Unverified)
}

/// The state's stable lowercase token. `ds` reports states in snake_case
/// everywhere; the engine's `Debug` spelling is an implementation detail.
const fn state_token(state: CapabilityState) -> &'static str {
    match state {
        CapabilityState::Ready => "ready",
        CapabilityState::Unsupported => "unsupported",
        CapabilityState::NotImplemented => "not_implemented",
        CapabilityState::Unverified => "unverified",
    }
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let raw_sources = inputs.repeated("source");
    let mut candidates = Vec::with_capacity(raw_sources.len());
    let mut total_bytes: u64 = 0;
    let mut total_files: usize = 0;

    for raw in raw_sources {
        let path = Path::new(raw);
        let metadata = std::fs::metadata(path).map_err(|error| {
            Failure::invalid("source_not_found", format!("cannot read `{raw}`"))
                .remedy("check each path; a directory is read as one folder source")
                .detail(json!({ "detail": error.kind().to_string() }))
        })?;

        if metadata.is_dir() {
            let members = read_folder(path, &mut total_bytes, &mut total_files)?;
            candidates.push(SourceCandidate::folder(display_name(path), members));
        } else {
            total_files += 1;
            total_bytes += metadata.len();
            check_bounds(total_bytes, total_files)?;
            let bytes = std::fs::read(path).map_err(|error| {
                Failure::failed("source_unreadable", format!("cannot read `{raw}`"))
                    .remedy("check file permissions")
                    .detail(json!({ "detail": error.kind().to_string() }))
            })?;
            candidates.push(SourceCandidate::file(display_name(path), bytes));
        }
    }

    let sources = SourceSet::new(candidates);
    let inspection = inspect_sources(&sources);
    let capabilities = conversion_capabilities(&inspection);

    let show_blocked = inputs.switch("blocked");
    let capability_values: Vec<Value> = capabilities
        .iter()
        .filter(|capability| show_blocked || offerable(capability.state))
        .map(|capability| {
            json!({
                "id": capability.id,
                "state": state_token(capability.state),
                "reason": capability.reason,
            })
        })
        .collect();

    let candidates: Vec<Value> = inspection
        .candidates
        .iter()
        .map(|candidate| {
            json!({
                "name": candidate.display_name,
                "kind": format!("{:?}", candidate.kind),
                "digest": candidate.digest,
                "members": candidate.member_count,
                "version_evidence": candidate.version_evidence,
                "units_evidence": candidate.units_evidence,
                "counts": candidate.counts,
            })
        })
        .collect();

    let mut answer = json!({
        "sources": candidates,
        "byte_len": total_bytes,
        "capabilities": capability_values,
    });

    if !show_blocked {
        let blocked = capabilities
            .iter()
            .filter(|capability| !offerable(capability.state))
            .count();
        if blocked > 0 {
            answer["more"] = json!({
                "blocked": blocked,
                "next": "ds network convert inspect --blocked",
            });
        }
    }

    Ok(answer)
}

/// Read a directory as one folder source, in a deterministic order.
///
/// Order matters: the engine digests the member list, so two runs over the
/// same tree must produce the same digest. Directory iteration order is not
/// guaranteed by the OS, so it is sorted here.
fn read_folder(
    root: &Path,
    total_bytes: &mut u64,
    total_files: &mut usize,
) -> Result<Vec<(String, Vec<u8>)>, Failure> {
    let mut members = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let mut paths: Vec<PathBuf> = Vec::new();

    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|error| {
            Failure::failed(
                "source_unreadable",
                format!("cannot list `{}`", dir.display()),
            )
            .remedy("check directory permissions")
            .detail(json!({ "detail": error.kind().to_string() }))
        })?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                paths.push(path);
            }
        }
    }
    paths.sort();

    for path in paths {
        let metadata = std::fs::metadata(&path).map_err(|error| {
            Failure::failed(
                "source_unreadable",
                format!("cannot read `{}`", path.display()),
            )
            .detail(json!({ "detail": error.kind().to_string() }))
        })?;
        *total_files += 1;
        *total_bytes += metadata.len();
        check_bounds(*total_bytes, *total_files)?;

        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = std::fs::read(&path).map_err(|error| {
            Failure::failed(
                "source_unreadable",
                format!("cannot read `{}`", path.display()),
            )
            .remedy("check file permissions")
            .detail(json!({ "detail": error.kind().to_string() }))
        })?;
        members.push((relative, bytes));
    }

    Ok(members)
}

fn check_bounds(total_bytes: u64, total_files: usize) -> Result<(), Failure> {
    if total_bytes > MAX_TOTAL_BYTES || total_files > MAX_FILES {
        return Err(
            Failure::invalid("source_too_large", "the sources exceed the read bound")
                .remedy("inspect a narrower subtree")
                .detail(json!({
                    "byte_len": total_bytes,
                    "files": total_files,
                    "max_byte_len": MAX_TOTAL_BYTES,
                    "max_files": MAX_FILES,
                })),
        );
    }
    Ok(())
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} source(s) · {} bytes\n\n",
        data["sources"].as_array().map_or(0, Vec::len),
        data["byte_len"],
    );
    for source in data["sources"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<28} {:<22} {} member(s)\n",
            source["name"].as_str().unwrap_or(""),
            source["kind"].as_str().unwrap_or(""),
            source["members"],
        ));
        out.push_str(&format!(
            "  {:<28} {}\n",
            "",
            source["digest"].as_str().unwrap_or("")
        ));
        for key in ["version_evidence", "units_evidence"] {
            if let Some(evidence) = source[key].as_str() {
                out.push_str(&format!("  {:<28} {key}: {evidence}\n", ""));
            }
        }
    }

    out.push_str("\nCAPABILITIES\n");
    let capabilities = data["capabilities"].as_array();
    match capabilities.filter(|list| !list.is_empty()) {
        Some(list) => {
            for capability in list {
                out.push_str(&format!(
                    "  {:<34} {}\n",
                    capability["id"].as_str().unwrap_or(""),
                    capability["state"].as_str().unwrap_or(""),
                ));
                if let Some(reason) = capability["reason"]
                    .as_str()
                    .filter(|text| !text.is_empty())
                {
                    out.push_str(&format!("  {:<34}   {reason}\n", ""));
                }
            }
        }
        None => out.push_str("  none available from this source set\n"),
    }

    if let Some(blocked) = data["more"]["blocked"].as_u64() {
        out.push_str(&format!(
            "\n{blocked} blocked — see `ds network convert inspect --blocked`\n"
        ));
    }
    out
}
