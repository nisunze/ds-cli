//! Turning `--source` paths into staged inputs, once.
//!
//! All three commands in this domain start the same way: read the operator's
//! paths into bytes and stage them for the Network addon as one ordered
//! source set. Doing that in one place is not only less code — it is the
//! only way the domain's refusals stay identical. A caller who learns
//! `source_too_large` from `ds dsgrid-exchange inspect` must get the same
//! code, with the same remedy, from `plan` and from `convert`; otherwise
//! "inspect first, then convert" stops being a reliable sequence.
//!
//! The directory walk itself lives in `ds_cli_dsgrid::folder`, because
//! `ds dsgrid model link` reads the same folder and must digest it
//! identically — see that module for why the order is fixed. The addon
//! receives each member under a label unique across the call (`<source
//! index>/<member>`), never a host path; it digests and classifies them.

use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Refusal};
use ds_network_contracts::grid_exchange::{ExchangeMemberV1, ExchangeSourceV1};
use serde_json::json;

/// A folder source is read whole. The bound is the same one the reference
/// closure task applies to a workspace, for the same reason: a mistyped path
/// at a large tree should fail in a moment, not after reading it.
pub use ds_cli_dsgrid::folder::{MAX_FILES, MAX_TOTAL_BYTES, read_folder};

/// The `--source` input, declared once. Every command in this domain takes
/// exactly this argument with exactly this help line, so the three screens
/// describe one concept rather than three similar ones.
pub const SOURCE_ARG: Arg = Arg::repeated(
    "source",
    "<path>",
    "A file, or a directory read as one folder source. Repeatable.",
)
.required();

/// The refusals every command in this domain shares. Declared here as data so
/// a command can splice them into its own list rather than restating them —
/// a restatement is a second description, and those drift.
pub const SHARED_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "source_not_found",
        when: "a --source path does not exist",
        remedy: "check each path; a directory is read as one folder source",
    },
    Refusal {
        code: "source_too_large",
        when: "the sources exceed the 512 MiB or 4096-file read bound",
        remedy: "convert a narrower subtree",
    },
    Refusal {
        code: "source_unreadable",
        when: "a source exists but cannot be read",
        remedy: "check file permissions",
    },
];

/// What was read, alongside the staged set. The byte and file totals are
/// reported rather than discarded: they are the cost model a caller needs to
/// decide whether the next call is worth making.
pub struct Loaded {
    /// The sources in `--source` order, each naming its staged members.
    pub sources: Vec<ExchangeSourceV1>,
    /// `(label, bytes)` in the same order, for the addon's inputs directory.
    pub inputs: Vec<(String, Vec<u8>)>,
    pub byte_len: u64,
    pub file_count: usize,
    /// One `streamed_volume` warning per source that sits on a streamed or
    /// network volume (contract 02 §7). A warning, never a refusal: the
    /// bytes were read; what the operator learns is that a pinned digest
    /// over them is only as stable as the stream.
    pub warnings: Vec<String>,
}

pub fn load(raw_sources: &[String]) -> Result<Loaded, Failure> {
    let mut sources = Vec::with_capacity(raw_sources.len());
    let mut inputs = Vec::new();
    let mut byte_len: u64 = 0;
    let mut file_count: usize = 0;
    let mut warnings = Vec::new();

    for (index, raw) in raw_sources.iter().enumerate() {
        let path = Path::new(raw.as_str());
        if let Some(hint) = ds_cli_dsgrid::folder::streamed_volume_hint(path) {
            warnings.push(format!("streamed_volume: {hint}"));
        }
        let metadata = std::fs::metadata(path).map_err(|error| {
            Failure::invalid("source_not_found", format!("cannot read `{raw}`"))
                .remedy("check each path; a directory is read as one folder source")
                .detail(json!({ "detail": error.kind().to_string() }))
        })?;
        let display_name = display_name(path);

        let (folder, members) = if metadata.is_dir() {
            (true, read_folder(path, &mut byte_len, &mut file_count)?)
        } else {
            file_count += 1;
            byte_len += metadata.len();
            ds_cli_dsgrid::folder::check_bounds(byte_len, file_count)?;
            let bytes = std::fs::read(path).map_err(|error| {
                Failure::failed("source_unreadable", format!("cannot read `{raw}`"))
                    .remedy("check file permissions")
                    .detail(json!({ "detail": error.kind().to_string() }))
            })?;
            (false, vec![(display_name.clone(), bytes)])
        };
        let mut declared = Vec::with_capacity(members.len());
        for (name, bytes) in members {
            let input = format!("{index}/{name}");
            declared.push(ExchangeMemberV1 {
                name,
                input: input.clone(),
            });
            inputs.push((input, bytes));
        }
        sources.push(ExchangeSourceV1 {
            display_name,
            folder,
            members: declared,
        });
    }

    Ok(Loaded {
        sources,
        inputs,
        byte_len,
        file_count,
        warnings,
    })
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}
