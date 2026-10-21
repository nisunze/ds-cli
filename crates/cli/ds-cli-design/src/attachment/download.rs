//! `ds design attachment download` — authorize one revision's bytes, or fetch
//! and verify them into a new local file.

use std::io::Write;
use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

pub const ATTACHMENT_ARG: Arg = Arg {
    name: "attachment",
    kind: ArgKind::Value,
    value: "<attachment-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "The logical file, from `ds design attachment list`.",
};

pub const REVISION_ARG: Arg = Arg {
    name: "revision",
    kind: ArgKind::Value,
    value: "<revision-id>",
    required: false,
    default: None,
    choices: &[],
    summary: "One exact revision. Omit for the file's current latest.",
};

const OUT_ARG: Arg = Arg::value(
    "out",
    "<file>",
    "Fetch, verify and write to this new file; never overwritten. Omit for the signed URL.",
);

pub const OUTPUT_REFUSED: Refusal = Refusal {
    code: "attachment_output_invalid",
    when: "--out already exists, or the verified bytes cannot be written there",
    remedy: "Name a new file in a writable directory; an existing file is never replaced.",
};

pub static COMMAND: Command = Command {
    id: "design.attachment.download",
    path: &["design", "attachment", "download"],
    contract: 1,
    summary: "Download one attachment revision, or authorize its signed URL.",
    purpose: "Authorize one immutable revision for the explicit project. With --out, fetch it through the server-signed, generation-pinned read, verify size and SHA-256, and write a new local file; the URL never leaves the client. Without --out, return the signed URL and digest to fetch and verify. Credentials are never sent to Storage.",
    chapter: Chapter::Design,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::versions::PROJECT,
        crate::transformer::LANE_ARG,
        ATTACHMENT_ARG,
        REVISION_ARG,
        OUT_ARG,
    ],
    output: "`attachment`, `revision`, `file`, `bytes` and `digest`; with --out also `out` and `verified`, without it the signed `url` and `expiresAt`.",
    examples: &[Example {
        command: "ds design attachment download --project <id> --attachment att-line-a-bak --revision rev_2 --out ./LINE_A_v2.bak --output json",
        note: "Written only after the SHA-256 matches .data.digest. Omit --out for the expiring URL.",
        runnable: false,
    }],
    refusals: &[super::NATIVE_REFUSED, OUTPUT_REFUSED],
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn output_failure(why: impl std::fmt::Display) -> Failure {
    Failure::invalid(OUTPUT_REFUSED.code, why.to_string()).remedy(OUTPUT_REFUSED.remedy)
}

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let attachment = inputs.require("attachment")?;
    let revision = inputs.value("revision");
    let Some(out) = inputs.value("out") else {
        return super::ask(
            inputs,
            ds_client_core::design_attachments::Command::Download {
                attachment: attachment.into(),
                revision: revision.map(str::to_owned),
            },
        );
    };
    let out = Path::new(out);
    // Refused before a byte is authorized, and again atomically at the write.
    if std::fs::symlink_metadata(out).is_ok() {
        return Err(output_failure(format!("{} already exists", out.display())));
    }
    let (mut receipt, bytes) = ds_cli_auth::design_attachment_bytes_for_project(
        inputs.require("lane")?,
        inputs.require("project")?,
        attachment,
        revision,
    )
    .map_err(super::refused)?;
    write_new(out, &bytes)?;
    receipt["out"] = json!(out);
    Ok(receipt)
}

/// Stage the verified bytes beside the destination and persist them without
/// replacing anything: a file that appeared while the bytes were fetched is
/// still never overwritten, and a half-written file never carries its name.
fn write_new(out: &Path, bytes: &[u8]) -> Result<(), Failure> {
    let parent = out
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(output_failure)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(output_failure)?;
    staged.write_all(bytes).map_err(output_failure)?;
    staged.as_file().sync_all().map_err(output_failure)?;
    staged
        .persist_noclobber(out)
        .map_err(|error| output_failure(error.error))?;
    Ok(())
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} · {} · {} bytes\n",
        data["file"].as_str().unwrap_or("?"),
        data["revision"].as_str().unwrap_or("?"),
        data["bytes"].as_u64().unwrap_or(0),
    );
    if let Some(path) = data["out"].as_str() {
        out.push_str(&format!(
            "  wrote {path} · sha256 {} verified\n",
            data["digest"].as_str().unwrap_or("?")
        ));
    }
    if let Some(url) = data["url"].as_str() {
        out.push_str(&format!("  {url}\n"));
    }
    if let Some(expires) = data["expiresAt"].as_str() {
        out.push_str(&format!("  expires {expires}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verified_download_never_replaces_an_existing_file() {
        let root = tempfile::tempdir().expect("temp dir");
        let out = root.path().join("nested").join("LINE_A_v2.bak");
        write_new(&out, b"first").expect("a new file is written");
        assert_eq!(std::fs::read(&out).expect("written"), b"first");
        let refused = write_new(&out, b"second").expect_err("an existing file is kept");
        assert_eq!(refused.code(), OUTPUT_REFUSED.code);
        assert_eq!(std::fs::read(&out).expect("kept"), b"first");
        // Nothing staged is left behind beside the destination.
        let names: Vec<_> = std::fs::read_dir(out.parent().expect("parent"))
            .expect("listing")
            .map(|entry| entry.expect("entry").file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("LINE_A_v2.bak")]);
    }
}
