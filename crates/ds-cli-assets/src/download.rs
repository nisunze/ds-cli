//! A fresh authorized read reference, without fetching the asset's bytes.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

use crate::{ASSET_ARG, CatalogueCommand, LANE_ARG, PROJECT_ARG};

pub static COMMAND: Command = Command {
    id: "assets.download",
    path: &["assets", "download"],
    contract: 1,
    summary: "Authorize a fresh read reference for one project file.",
    purpose: "Return the exact asset row, its short-lived signed download URL and expiry after the existing project and sensitivity checks. No asset bytes are fetched, so large PDFs can be streamed by their document reader without the assets.read memory limit. The URL is temporary read authority: use it immediately and do not retain it as a durable link. A projected sys: row is refused before authentication. No URL, storage path or credential is accepted.",
    chapter: Chapter::Assets,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[ASSET_ARG, LANE_ARG, PROJECT_ARG],
    output: "asset (the exact project-owned row), download_url and expires_at. JSON includes the temporary signed URL; human output shows metadata only. No byte download or project change.",
    examples: &[Example {
        command: "ds assets download --project <exact-id> --asset a_7kq3nr2v0b1c --output json",
        note: "Consume the signed reference immediately with the document reader; verify the asset identity and digest against its catalogue row.",
        runnable: false,
    }],
    refusals: &crate::refusals::<25>(&[
        crate::INVALID_ASSET_ID,
        crate::ORIGIN_READ_UNAVAILABLE,
        crate::ASSETS_UNREADABLE,
    ]),
    reference: Some("docs/reference/assets.md"),
    search: &["signed read", "large pdf", "download reference"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn request(inputs: &Inputs) -> Result<CatalogueCommand, Failure> {
    let asset_id = crate::asset_id(inputs.require("asset")?, "asset")?;
    if crate::is_projected(&asset_id) {
        return Err(crate::projected_unavailable(&asset_id));
    }
    Ok(CatalogueCommand::Download { asset_id })
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request = request(inputs)?;
    crate::catalogue(
        inputs.value("lane").unwrap_or("stable"),
        inputs.require("project")?,
        &request,
    )
}

pub fn render(data: &Value) -> String {
    format!(
        "{} · {}\n  {} · expires {}\n",
        crate::truncate(data["asset"]["name"].as_str().unwrap_or("?"), 80),
        data["asset"]["asset_id"].as_str().unwrap_or("?"),
        data["asset"]["digest"].as_str().unwrap_or("—"),
        data["expires_at"].as_str().unwrap_or("?"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::args::parse;
    use serde_json::json;

    fn inputs(asset: &str) -> Inputs {
        parse(
            &COMMAND,
            &["--project", "p", "--asset", asset].map(str::to_owned),
        )
        .unwrap()
    }

    #[test]
    fn large_file_read_reference_uses_the_existing_typed_download_only() {
        assert_eq!(
            request(&inputs("a_4v6w339ffevs")).unwrap(),
            CatalogueCommand::Download {
                asset_id: "a_4v6w339ffevs".into()
            }
        );
        assert_eq!(COMMAND.effect, Effect::ReadOnly);
        assert!(!COMMAND.confirmation_required_for(&inputs("a_4v6w339ffevs")));
        assert!(parse(&COMMAND, &["--asset".into(), "a_4v6w339ffevs".into()]).is_err());
        assert!(
            parse(
                &COMMAND,
                &[
                    "--project",
                    "p",
                    "--asset",
                    "a_4v6w339ffevs",
                    "--url",
                    "https://example.com"
                ]
                .map(str::to_owned)
            )
            .is_err()
        );
    }

    #[test]
    fn invalid_and_projected_assets_refuse_before_native_restoration() {
        assert_eq!(
            request(&inputs("not_an_asset")).unwrap_err().code(),
            "invalid_asset_id"
        );
        assert_eq!(
            request(&inputs("sys:grid_export:model:export:pdf"))
                .unwrap_err()
                .code(),
            "origin_read_unavailable"
        );
    }

    #[test]
    fn human_output_never_prints_the_signed_read_authority() {
        let data = json!({"asset":{"asset_id":"a_4v6w339ffevs","name":"MV.pdf","bytes":96011002,"digest":"sha256:abc"},
            "download_url":"https://storage.googleapis.com/bucket/MV.pdf?secret=opaque",
            "expires_at":"2026-10-06T01:00:00Z"});
        let text = render(&data);
        assert!(text.contains("MV.pdf"));
        assert!(text.contains("a_4v6w339ffevs"));
        assert!(!text.contains("opaque"));
        assert!(!text.contains("https://"));
    }
}
