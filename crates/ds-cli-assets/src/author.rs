//! Direct bounded Markdown, HTML and plain text authoring through Project Assets.
use crate::{CatalogueCommand, FOLDER_ARG, LANE_ARG, PROJECT_ARG};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_assets::{IngestRequest, RECOGNISE_HEAD_BYTES};
use ds_command_kernel::assets::Format;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::Cursor;

const MAX_AUTHORED_BYTES: usize = 64 * 1024;
const BAD_CONTENT: Refusal = Refusal {
    code: "invalid_authored_content",
    when: "the name is not a simple .md, .html or .txt filename, content is empty or above 64 KiB, or its bytes contradict the named format",
    remedy: "give one filename ending .md, .html or .txt and matching nonempty UTF-8 content of at most 64 KiB",
};

pub static COMMAND: Command = Command {
    id: "assets.author",
    path: &["assets", "author"],
    contract: 1,
    summary: "Author a Markdown or HTML report directly as a project attachment.",
    purpose: "Creates one bounded text asset from --content, so an MCP caller can submit a complete Markdown or HTML project report without writing a temporary file. The filename and bytes must agree. The asset is internal unless its folder sets a stricter default or a stricter sensitivity is named. This uses the same governed catalogue and uploader as assets.ingest. To place it on a PM project note or task, use assets.attach with the returned asset_id; read the record or task back to verify the link. It never changes an existing attachment.",
    chapter: Chapter::Assets,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "name",
            "<filename.md|.html|.txt>",
            "Filename displayed to the user; no path.",
        )
        .required(),
        Arg::value(
            "content",
            "<utf-8-text>",
            "Complete Markdown, HTML document or plain text, up to 64 KiB.",
        )
        .required(),
        FOLDER_ARG,
        Arg::value(
            "sensitivity",
            "<class>",
            "Optional access class; never inferred as open.",
        )
        .choices(crate::SENSITIVITIES),
        LANE_ARG,
        PROJECT_ARG,
    ],
    output: "The created asset row and recognised format; its asset_id can be linked with assets.attach.",
    examples: &[Example {
        command: "ds assets author --project <exact-id> --name Review.md --content '# Review' --folder reviews/2026 --yes",
        note: "Then attach the returned asset_id to the PM record with assets.attach.",
        runnable: false,
    }],
    refusals: &crate::refusals::<27>(&[
        BAD_CONTENT,
        crate::INVALID_FOLDER_PATH,
        crate::CONFIRMATION_REQUIRED,
        crate::UNKNOWN_FOLDER,
        crate::ASSETS_UNREADABLE,
    ]),
    reference: Some("docs/reference/assets.md"),
    search: &[
        "write markdown project note attachment",
        "author html report asset",
        "attach report to PM task or note",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn prepared(inputs: &Inputs) -> Result<(String, Vec<u8>, Format), Failure> {
    let name = inputs.require("name")?.trim();
    if name.len() > 160
        || name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
    {
        return Err(Failure::invalid(
            BAD_CONTENT.code,
            "The report name must be one simple filename.",
        )
        .remedy(BAD_CONTENT.remedy));
    }
    let expected = if name.to_ascii_lowercase().ends_with(".md") {
        Format::Md
    } else if name.to_ascii_lowercase().ends_with(".html") {
        Format::Html
    } else if name.to_ascii_lowercase().ends_with(".txt") {
        Format::Txt
    } else {
        return Err(
            Failure::invalid(BAD_CONTENT.code, "Use a .md, .html or .txt filename.")
                .remedy(BAD_CONTENT.remedy),
        );
    };
    let bytes = inputs.require("content")?.as_bytes().to_vec();
    if bytes.is_empty() || bytes.len() > MAX_AUTHORED_BYTES {
        return Err(Failure::invalid(
            BAD_CONTENT.code,
            "Report content must be between 1 byte and 64 KiB.",
        )
        .remedy(BAD_CONTENT.remedy));
    }
    let found = ds_command_kernel::assets::recognise(
        Some(name),
        &bytes[..bytes.len().min(RECOGNISE_HEAD_BYTES)],
        bytes.len() as u64,
    );
    if found.format != expected {
        return Err(Failure::invalid(
            BAD_CONTENT.code,
            format!(
                "The filename says {expected}, but the content is {}.",
                found.format
            ),
        )
        .remedy(BAD_CONTENT.remedy));
    }
    Ok((name.to_string(), bytes, expected))
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let (name, bytes, format) = prepared(inputs)?;
    let lane = inputs.value("lane").unwrap_or("stable");
    let project = inputs.require("project")?;
    let folder_id = match inputs.value("folder") {
        Some(raw) => Some(
            crate::folder_at(lane, project, &crate::folder_path(raw, "folder")?)?["folder_id"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        ),
        None => None,
    };
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let recognised = ds_command_kernel::assets::recognise(
        Some(&name),
        &bytes[..bytes.len().min(RECOGNISE_HEAD_BYTES)],
        bytes.len() as u64,
    );
    let request = IngestRequest {
        name,
        size: bytes.len() as u64,
        sha256: digest.clone(),
        content_type: Some(
            match format {
                Format::Md => "text/markdown; charset=utf-8",
                Format::Html => "text/html; charset=utf-8",
                _ => "text/plain; charset=utf-8",
            }
            .into(),
        ),
        folder_id,
        sensitivity: inputs.value("sensitivity").map(str::to_owned),
        kind: crate::enum_token(&recognised.kind),
        format: crate::enum_token(&recognised.format),
    };
    let mut source = Cursor::new(bytes);
    let report = ds_cli_auth::project_assets_for_project(
        lane,
        project,
        &CatalogueCommand::Ingest(request),
        Some(&mut source),
    )?;
    let mut answer = report.into_result();
    answer["bytes"] = json!(source.get_ref().len());
    answer["digest"] = json!(format!("sha256:{digest}"));
    answer["recognised"] = serde_json::to_value(&recognised)
        .map_err(|error| Failure::internal(crate::ASSETS_UNREADABLE.code, error.to_string()))?;
    Ok(answer)
}

pub fn render(data: &Value) -> String {
    let row = &data["asset"];
    format!(
        "authored {}\n{}",
        row["asset_id"].as_str().unwrap_or("?"),
        crate::asset_line(row)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(name: &str, content: &str) -> Inputs {
        ds_cli_contract::parse(
            &COMMAND,
            &[
                "--name".into(),
                name.into(),
                "--content".into(),
                content.into(),
                "--project".into(),
                "test".into(),
            ],
        )
        .unwrap()
    }
    #[test]
    fn authored_content_requires_a_matching_document_and_bounded_name() {
        assert_eq!(
            prepared(&parse(
                "Report.html",
                "<!doctype html><html><body>Done</body></html>"
            ))
            .unwrap()
            .2,
            Format::Html
        );
        assert_eq!(
            prepared(&parse("Report.md", "# Done")).unwrap().2,
            Format::Md
        );
        for (name, content) in [
            ("Report.html", "# Not HTML"),
            ("../Report.html", "<html></html>"),
            ("Report.pdf", "# Not a PDF"),
            ("Report.md", ""),
        ] {
            assert_eq!(
                prepared(&parse(name, content)).unwrap_err().code(),
                BAD_CONTENT.code
            );
        }
    }
}
