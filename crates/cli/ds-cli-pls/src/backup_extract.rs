//! Exact-byte backup recovery, over the host-neutral native task.
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_grid_tasks::{ExtractPlsBackupRequest, extract_pls_backup};
use serde_json::Value;

pub static COMMAND: Command = Command {
    id: "pls.backup-extract",
    path: &["pls", "backup-extract"],
    contract: 1,
    summary: "List or recover exact native bytes from a PLS-CADD backup.",
    purpose: "Inventory one digest-pinned raw or ZIP-wrapped backup. Supply an exact inventory member path and a fresh output file of the same leaf to recover its exact bytes. No normalization, path healing, structure synthesis or native edits occur. A bounded inventory reports omitted members; extraction returns the selected member's SHA-256 and byte count.",
    chapter: Chapter::PlsCadd,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("backup", "<path>", "Raw or ZIP-wrapped native backup.").required(),
        Arg::value(
            "source-sha256",
            "<sha256:hex>",
            "Expected exact complete backup digest.",
        )
        .required(),
        Arg::value(
            "member",
            "<inventory-path>",
            "Exact case-sensitive member path; requires --out.",
        ),
        Arg::value(
            "out",
            "<fresh-file>",
            "New file with the exact native member leaf; requires --member.",
        ),
        Arg::value(
            "limit",
            "<1..4096>",
            "Cap the inventory; ignored for one selected member.",
        )
        .default("50"),
    ],
    output: "source_path, source_sha256, source_bytes, member_count, bounded members {member, leaf, sha256, byte_len, native_type}, omitted_members, written, member_bytes_preserved and path_healing_performed=false.",
    examples: &[],
    refusals: &[
        Refusal {
            code: "source_not_found",
            when: "--backup is not a readable regular file",
            remedy: "name the exact native backup",
        },
        Refusal {
            code: "output_exists",
            when: "--out already exists",
            remedy: "choose a new file with the same native leaf",
        },
        Refusal {
            code: "output_write_failed",
            when: "the output cannot be resolved",
            remedy: "choose an absolute new output file under an existing writable directory",
        },
        Refusal {
            code: "invalid_limit",
            when: "--limit is not a whole number in 1..4096",
            remedy: "use a limit from 1 through 4096",
        },
        Refusal {
            code: "task_refused",
            when: "the native task refuses the digest, member selection, output leaf or extraction",
            remedy: "read detail.code and detail.detail; pin the exact backup digest and select one exact inventory member path",
        },
        crate::RESULT_ENCODING_REFUSAL,
    ],
    reference: Some("docs/reference/pls.md"),
    search: &["archive", "native source", "structure files"],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = inputs
        .value("limit")
        .unwrap_or("50")
        .parse::<usize>()
        .ok()
        .filter(|n| (1..=4096).contains(n))
        .ok_or_else(|| {
            Failure::invalid("invalid_limit", "limit must be in 1..4096")
                .remedy("use a limit from 1 through 4096")
        })?;
    let request = ExtractPlsBackupRequest {
        source_backup_path: crate::source_path(inputs.require("backup")?, "backup")?,
        expected_source_sha256: inputs.require("source-sha256")?.into(),
        member: inputs.value("member").map(str::to_string),
        output_file: inputs.value("out").map(crate::output_path).transpose()?,
        limit,
    };
    let result =
        extract_pls_backup(&request).map_err(|e| crate::task_failure(&e.code, &e.detail))?;
    crate::encode(&result)
}

pub fn render(data: &Value) -> String {
    format!(
        "PLS-CADD exact backup members: {} ({} omitted)\n  written {}\n",
        data["member_count"], data["omitted_members"], data["written"]
    )
}
