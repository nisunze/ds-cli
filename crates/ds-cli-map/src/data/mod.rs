//! Project GIS file lifecycle. File IO is a host effect; ingestion remains server-owned.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};
const PATH: Arg = Arg::value(
    "path",
    "<file>",
    "GIS source file (zip shapefiles with their sidecars).",
)
.required();
const SHA: Arg = Arg::value(
    "sha256",
    "<digest>",
    "Require the exact bytes from a previous offline inspection.",
);
const FILE: Refusal = Refusal {
    code: "gis_file_invalid",
    when: "the source is not a readable regular file of 1 byte to 1 GiB, or its digest differs",
    remedy: "check the local file and inspect its bytes before upload",
};
const LIMIT: Arg = Arg::value("limit", "<n>", "At most 1..500 upload records.").default("100");
const LANE: Arg = super::layer::native::LANE_ARG;
const REFUSALS: &[Refusal] = super::layer::native::NATIVE_WRITE_REFUSALS;
const UPLOAD_REFUSALS: [Refusal; REFUSALS.len() + 2] = {
    let mut list = [FILE; REFUSALS.len() + 2];
    let mut i = 0;
    while i < REFUSALS.len() {
        list[i] = REFUSALS[i];
        i += 1;
    }
    list[REFUSALS.len() + 1] = super::INVALID_NUMBER;
    list
};
fn bad(message: impl Into<String>) -> Failure {
    Failure::invalid("gis_file_invalid", message).remedy(FILE.remedy)
}
fn snapshot(inputs: &Inputs) -> Result<(tempfile::NamedTempFile, Value), Failure> {
    let path = std::path::Path::new(inputs.require("path")?);
    let mut source = std::fs::File::open(path).map_err(|_| bad("GIS source cannot be opened"))?;
    let meta = source
        .metadata()
        .map_err(|_| bad("GIS source metadata is unavailable"))?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > ds_cli_auth::MAX_UPLOAD_BYTES {
        return Err(bad("GIS file must be a regular file of 1 byte to 1 GiB"));
    }
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty() && s.len() <= 500 && !s.chars().any(char::is_control))
        .ok_or_else(|| bad("GIS filename is invalid"))?;
    let mut staged =
        tempfile::NamedTempFile::new().map_err(|_| bad("GIS staging file cannot be created"))?;
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = source
            .read(&mut buffer)
            .map_err(|_| bad("GIS source read failed"))?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        if bytes > ds_cli_auth::MAX_UPLOAD_BYTES {
            return Err(bad("GIS source exceeds 1 GiB"));
        }
        hash.update(&buffer[..n]);
        staged
            .write_all(&buffer[..n])
            .map_err(|_| bad("GIS staging write failed"))?;
    }
    if bytes != meta.len() {
        return Err(bad("GIS source size changed while staging"));
    }
    let digest = format!("{:x}", hash.finalize());
    if inputs
        .value("sha256")
        .is_some_and(|expected| expected != digest)
    {
        return Err(bad("GIS source differs from the requested SHA-256"));
    }
    staged
        .as_file_mut()
        .seek(SeekFrom::Start(0))
        .map_err(|_| bad("GIS staging seek failed"))?;
    Ok((
        staged,
        json!({"file_name":name,"bytes":bytes,"sha256":digest,"uploaded":false,"validated":"byte_inventory"}),
    ))
}
pub mod inspect {
    use super::*;
    pub static COMMAND: Command = Command {
        id: "map.data.inspect",
        path: &["map", "data", "inspect"],
        contract: 1,
        summary: "Inspect GIS file bytes offline before upload.",
        purpose: "Computes a bounded file inventory and SHA-256 without sign-in, a desktop or network. This inspects bytes; the governed uploader owns parsing and tiling. Use design upload inspection for canonical design cleaning.",
        chapter: Chapter::Survey,
        effect: Effect::ReadOnly,
        authority: Authority::None,
        execution: Execution::Sync,
        args: &[PATH, SHA],
        output: "Filename, byte size and SHA-256; uploaded false. No claim of geometry validation or cleaning.",
        examples: &[Example {
            command: "ds map data inspect --path ./roads.geojson --output json",
            note: "Works offline.",
            runnable: false,
        }],
        refusals: &[FILE],
        reference: Some("docs/reference/map.md"),
        availability: super::super::layer::local_availability,
    };
    pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
        snapshot(inputs).map(|(_, data)| data)
    }
    pub fn render(data: &Value) -> String {
        format!(
            "{} · {} bytes\nSHA-256 {}\n",
            data["file_name"].as_str().unwrap_or("?"),
            data["bytes"],
            data["sha256"].as_str().unwrap_or("?")
        )
    }
}
pub mod upload {
    use super::*;
    pub static COMMAND: Command = Command {
        id: "map.data.upload",
        path: &["map", "data", "upload"],
        contract: 1,
        summary: "Upload project GIS data without a desktop (needs --yes).",
        purpose: "Stages exact source bytes locally, obtains a server-issued upload session, streams the file, and registers it for project tiling. Requires a reachable backend and project edit permission. An accepted file is reported separately from ready map tiles; tiling failure remains visible in the receipt.",
        chapter: Chapter::Survey,
        effect: Effect::GlobalWrite,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args: &[PATH, SHA, LANE],
        output: "Project, upload id, filename, transferred bytes, SHA-256, registered, tile_status and ready. Session URLs and storage paths are withheld.",
        examples: &[Example {
            command: "ds map data upload --path ./roads.geojson --yes",
            note: "The project is selected through native ds project use.",
            runnable: false,
        }],
        refusals: &UPLOAD_REFUSALS,
        reference: Some("docs/reference/map.md"),
        availability: ds_cli_auth::native_availability,
    };
    pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
        let (mut staged, info) = snapshot(inputs)?;
        let mut result = ds_cli_auth::project_data(
            inputs.require("lane")?,
            ds_cli_auth::ProjectDataCommand::Upload {
                file_name: info["file_name"].as_str().expect("filename"),
                size: info["bytes"].as_u64().expect("size"),
                reader: staged.as_file_mut(),
            },
        )?;
        result["sha256"] = info["sha256"].clone();
        Ok(result)
    }
    pub fn render(data: &Value) -> String {
        format!(
            "{} · registered: {} · tile status: {} · ready: {}\n",
            data["upload_id"].as_str().unwrap_or("?"),
            data["registered"],
            data["tile_status"].as_str().unwrap_or("?"),
            data["ready"]
        )
    }
}
pub mod list {
    use super::*;
    pub static COMMAND: Command = Command {
        id: "map.data.list",
        path: &["map", "data", "list"],
        contract: 1,
        summary: "List the selected project's uploaded GIS files and tiling status.",
        purpose: "Reads the project-owned upload catalogue through the native authenticated client. Does not require a desktop or open map.",
        chapter: Chapter::Survey,
        effect: Effect::LocalAuthState,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args: &[LANE, LIMIT],
        output: "Project, total uploads, bounded metadata rows and more. Signed URLs, raw storage paths and backend diagnostic strings are omitted.",
        examples: &[Example {
            command: "ds map data list --output json",
            note: "Check tile_status after uploading.",
            runnable: false,
        }],
        refusals: &UPLOAD_REFUSALS,
        reference: Some("docs/reference/map.md"),
        availability: ds_cli_auth::native_availability,
    };
    pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
        let limit = super::super::integer(inputs.require("limit")?, "limit", 1, 500)? as usize;
        let mut result = ds_cli_auth::project_data(
            inputs.require("lane")?,
            ds_cli_auth::ProjectDataCommand::List,
        )?;
        let total = result["total"].as_u64().unwrap_or(0);
        if let Some(rows) = result["uploads"].as_array_mut() {
            rows.truncate(limit)
        }
        result["more"] = json!(total > limit as u64);
        Ok(result)
    }
    pub fn render(data: &Value) -> String {
        let mut text = format!("{} project GIS uploads\n", data["total"]);
        for row in data["uploads"].as_array().into_iter().flatten() {
            text.push_str(&format!(
                "{} · {} · {}\n",
                row["upload_id"].as_str().unwrap_or("?"),
                row["file_name"].as_str().unwrap_or("?"),
                row["tile_status"].as_str().unwrap_or("unknown")
            ))
        }
        text
    }
}
pub mod remove {
    use super::*;
    pub static COMMAND: Command = Command {
        id: "map.data.remove",
        path: &["map", "data", "remove"],
        contract: 1,
        summary: "Delete a project GIS upload and its owned storage (needs --yes).",
        purpose: "Deletes one exact upload id returned by map data list. The backend owns authorization, linked-data handling and storage cleanup.",
        chapter: Chapter::Survey,
        effect: Effect::GlobalWrite,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args: &[
            LANE,
            Arg::value("upload", "<id>", "Exact id from map data list.").required(),
        ],
        output: "Project, upload_id and removed true, confirmed by the backend.",
        examples: &[Example {
            command: "ds map data remove --upload roads --yes",
            note: "Removes the selected project upload.",
            runnable: false,
        }],
        refusals: &UPLOAD_REFUSALS,
        reference: Some("docs/reference/map.md"),
        availability: ds_cli_auth::native_availability,
    };
    pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
        ds_cli_auth::project_data(
            inputs.require("lane")?,
            ds_cli_auth::ProjectDataCommand::Remove {
                upload_id: inputs.require("upload")?,
            },
        )
    }
    pub fn render(data: &Value) -> String {
        format!(
            "removed {} from {}\n",
            data["upload_id"].as_str().unwrap_or("?"),
            data["project"].as_str().unwrap_or("?")
        )
    }
}
