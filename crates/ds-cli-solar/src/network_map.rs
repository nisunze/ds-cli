//! Thin adapter: native media copy followed by the Solar owner's fenced save.
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Execution};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};
use std::io::Read;

pub static COMMAND: Command = Command {
    id: "solar.network.map",
    path: &["solar", "network", "map"],
    contract: 1,
    summary: "Copy a map into a Solar city's independent inputs.",
    purpose: "Upload one PNG, JPEG or WebP through Solar's verified media service, then save its immutable Solar-owned identity into the city's editable inputs. Existing project maps remain unchanged. Use network.map:reseau_propose for a city map or network.transformer:<name> for a transformer map. The cloud city must already exist. The expected city digest protects manual edits; a concurrent edit leaves the uploaded media unattached and the form unchanged. Run project sync afterwards to publish the changed inputs. No TypeScript, Desktop or source asset dependency.",
    chapter: Chapter::Solar,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        Arg::value("workspace", "<dir>", "Existing Solar workspace.").required(),
        Arg::value("city", "<id>", "Existing Solar city.").required(),
        Arg::value(
            "expected",
            "<digest>",
            "Current city digest from city read or network resolve.",
        )
        .required(),
        Arg::value(
            "role",
            "<role>",
            "network.map:<key> or network.transformer:<name>.",
        )
        .required(),
        Arg::value("file", "<file>", "Map image, at most 16 MiB.").required(),
        Arg::value("lane", "<stable|canary>", "Signed-in lane; default stable.")
            .choices(&["stable", "canary"]),
    ],
    output: "Solar-owned immutable media identity with byte hash; saved city digest and pending input sync receipt.",
    examples: &[],
    refusals: crate::network_form::COMMAND.refusals,
    reference: Some("docs/reference/solar.md"),
    availability: || crate::DS_SOLAR.availability(),
};
pub fn run(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let held = crate::project::invoke(
        json!({"operation":"network_form_read","workspace":i.require("workspace")?,"city":i.require("city")?}),
    )?;
    if held["expected"] != i.require("expected")? {
        return Err(Failure::invalid(
            "network_form",
            "City changed; read its current digest before copying a map.",
        ));
    }
    let path = std::path::Path::new(i.require("file")?);
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(file_error)?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(file_error)?;
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| file_error("Map filename is not UTF-8"))?;
    let mut session = ds_cli_auth::solar_project_session(i.value("lane").unwrap_or("stable"))?;
    if session.binding()["project"] != held["project_id"] {
        return Err(Failure::invalid(
            "asset_project",
            "Select the Solar workspace project before copying maps.",
        ));
    }
    let copy = session.execute(&ds_cli_auth::SolarProjectCommand::ImportMap {
        city: i.require("city")?.into(),
        role: i.require("role")?.into(),
        file_name: file_name.into(),
        bytes,
    })?;
    let mut saved = crate::project::invoke(
        json!({"operation":"network_map_save","workspace":i.require("workspace")?,"city":i.require("city")?,"expected":i.require("expected")?,"reference":copy["reference"],"file":path}),
    )?;
    saved["media_copy"] = copy;
    Ok(saved)
}
fn file_error(error: impl std::fmt::Display) -> Failure {
    Failure::invalid("form_file", error.to_string())
}
